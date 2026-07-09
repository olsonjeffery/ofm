use std::collections::HashMap;
use std::sync::Arc;

use axum::routing::post;
use axum::{Json, Router};
use hiqlite::params;
use jsonwebtoken::{encode, EncodingKey, Header};
use serde_json::{json, Value};
use tokio::sync::Mutex;
use uuid::Uuid;

use ofm::auth::api_key;
use ofm::auth::jwks::{base64url_encode, Claims, JwksCache};
use ofm::auth::AuthLayer;
use ofm::db;
use ofm::providers::LlmProvider;
use ofm::server;
use ofm::server::state::{AppState, OidcEndpoints};
use ofm::server::ws::bus::BroadcastBus;
use ofm::services::auth::{complete_onboarding, current_user};

fn make_jwt_cache() -> (Vec<u8>, String, JwksCache) {
    let key = b"test-hmac-secret-key-32-bytes-long!";
    let kid = "test-key-1";

    let jwk: jsonwebtoken::jwk::Jwk = serde_json::from_value(json!({
        "kty": "oct",
        "k": base64url_encode(key),
        "kid": kid,
        "alg": "HS256"
    }))
    .unwrap();

    let mut keys = HashMap::new();
    keys.insert(kid.to_string(), jwk);
    let cache = JwksCache {
        keys,
        issuer: "test-issuer".to_string(),
        client_id: "test-client".to_string(),
    };
    (key.to_vec(), kid.to_string(), cache)
}

fn make_jwt(key: &[u8], kid: &str, sub: &str) -> String {
    let claims = Claims {
        sub: sub.to_string(),
        iss: "test-issuer".to_string(),
        aud: json!("test-client"),
        exp: 9_999_999_999,
        iat: Some(1_000_000_000),
    };
    let mut header = Header::new(jsonwebtoken::Algorithm::HS256);
    header.kid = Some(kid.to_string());
    encode(&header, &claims, &EncodingKey::from_secret(key)).unwrap()
}

async fn make_client() -> (hiqlite::Client, tempfile::TempDir) {
    let tmp = tempfile::TempDir::new().unwrap();
    let config = hiqlite::NodeConfig {
        node_id: 1,
        nodes: vec![hiqlite::Node {
            id: 1,
            addr_raft: "127.0.0.1:0".into(),
            addr_api: "127.0.0.1:0".into(),
        }],
        data_dir: tmp.path().to_str().unwrap().to_string().into(),
        secret_raft: "test-raft-secret-123".into(),
        secret_api: "test-api-secret-123".into(),
        ..Default::default()
    };
    let client = hiqlite::start_node(config).await.unwrap();
    client.wait_until_healthy_db().await;
    db::run_migrations(&client).await.unwrap();
    (client, tmp)
}

async fn insert_user(
    client: &hiqlite::Client,
    id: Uuid,
    username: &str,
    oidc_subject: Option<&str>,
    is_admin: bool,
    is_active: bool,
) {
    let admin: i64 = if is_admin { 1 } else { 0 };
    let active: i64 = if is_active { 1 } else { 0 };
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    if let Some(sub) = oidc_subject {
        client
            .execute(
                "INSERT INTO users (id, username, oidc_subject, is_admin, is_active, created_at, has_completed_onboarding, is_technical) VALUES ($1, $2, $3, $4, $5, $6, 1, 0)",
                params!(id.to_string(), username, sub, admin, active, now),
            )
            .await
            .unwrap();
    } else {
        client
            .execute(
                "INSERT INTO users (id, username, is_admin, is_active, created_at, has_completed_onboarding, is_technical) VALUES ($1, $2, $3, $4, $5, 1, 0)",
                params!(id.to_string(), username, admin, active, now),
            )
            .await
            .unwrap();
    }
}

fn make_app_state(client: hiqlite::Client, user_id: Uuid, oidc: Option<OidcEndpoints>) -> AppState {
    AppState {
        cfg_port: 0,
        db: client,
        default_user_id: user_id,
        archive_root: "storage/".into(),
        config_root: "/tmp".into(),
        active_sessions: Arc::new(Mutex::new(HashMap::<String, Box<dyn LlmProvider>>::new())),
        oidc_provider: oidc,
        pkce_store: Arc::new(Mutex::new(HashMap::new())),
        cookie_key: cookie::Key::generate(),
        api_key_pepper: b"test_pepper".to_vec(),
        ws_bus: BroadcastBus::new(),
    }
}

#[tokio::test]
async fn test_health_check() {
    let (client, _tmp) = make_client().await;
    let user_id = db::ensure_default_user(&client).await.unwrap();
    let auth_layer = AuthLayer::disabled(
        client.clone(),
        b"test".to_vec(),
        cookie::Key::generate(),
        user_id,
    );
    let state = make_app_state(client, user_id, None);
    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let resp = reqwest::Client::new()
        .get(format!("http://{}/health", addr))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.text().await.unwrap(), "ok");
}

#[tokio::test]
async fn test_login_returns_authorization_url() {
    let (client, _tmp) = make_client().await;
    let user_id = db::ensure_default_user(&client).await.unwrap();
    let oidc = OidcEndpoints {
        end_session_endpoint: None,
        authorization_endpoint: "https://provider.test/auth".into(),
        token_endpoint: "https://provider.test/token".into(),
        revocation_endpoint: None,
        client_id: "test-client".into(),
        client_secret: None,
        redirect_uri: "http://localhost:3183/api/auth/callback".into(),
        jwks_cache: None,
        jwks_issuer: None,
    };
    let state = make_app_state(client, user_id, Some(oidc));
    let auth_layer = AuthLayer::disabled(
        state.db.clone(),
        b"test".to_vec(),
        state.cookie_key.clone(),
        state.default_user_id,
    );
    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let resp = reqwest::Client::new()
        .get(format!("http://{}/api/auth/login", addr))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    let auth_url = body["authorization_url"].as_str().unwrap();
    assert!(auth_url.contains("response_type=code"));
    assert!(auth_url.contains("code_challenge="));
    assert!(auth_url.contains("state="));
}

#[tokio::test]
async fn test_login_returns_400_when_oidc_disabled() {
    let (client, _tmp) = make_client().await;
    let user_id = db::ensure_default_user(&client).await.unwrap();
    let state = make_app_state(client, user_id, None);
    let auth_layer = AuthLayer::disabled(
        state.db.clone(),
        b"test".to_vec(),
        state.cookie_key.clone(),
        state.default_user_id,
    );
    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let resp = reqwest::Client::new()
        .get(format!("http://{}/api/auth/login", addr))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn test_callback_rejects_invalid_state() {
    let (client, _tmp) = make_client().await;
    let user_id = db::ensure_default_user(&client).await.unwrap();
    let oidc = OidcEndpoints {
        end_session_endpoint: None,
        authorization_endpoint: "https://provider.test/auth".into(),
        token_endpoint: "https://provider.test/token".into(),
        revocation_endpoint: None,
        client_id: "test-client".into(),
        client_secret: None,
        redirect_uri: "http://localhost:3183/api/auth/callback".into(),
        jwks_cache: None,
        jwks_issuer: None,
    };
    let state = make_app_state(client, user_id, Some(oidc));
    let auth_layer = AuthLayer::disabled(
        state.db.clone(),
        b"test".to_vec(),
        state.cookie_key.clone(),
        state.default_user_id,
    );
    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let resp = reqwest::Client::new()
        .get(format!(
            "http://{}/api/auth/callback?code=test_code&state=bad-state",
            addr
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn test_me_returns_401_without_token() {
    let (client, _tmp) = make_client().await;
    let user_id = db::ensure_default_user(&client).await.unwrap();
    let auth_layer = AuthLayer {
        enabled: true,
        db: client.clone(),
        jwks_cache: Arc::new(tokio::sync::RwLock::new(None)),
        issuer_url: None,
        client_id: None,
        pepper: b"test".to_vec(),
        cookie_key: cookie::Key::generate(),
        default_user_id: Uuid::nil(),
    };
    let state = make_app_state(client, user_id, None);
    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let resp = reqwest::Client::new()
        .get(format!("http://{}/api/auth/me", addr))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn test_me_returns_user() {
    let (client, _tmp) = make_client().await;
    let (key, kid, cache) = make_jwt_cache();
    let user_id = Uuid::new_v4();
    insert_user(
        &client,
        user_id,
        "testuser",
        Some("test-subject"),
        false,
        true,
    )
    .await;
    let auth_layer = AuthLayer {
        enabled: true,
        db: client.clone(),
        jwks_cache: Arc::new(tokio::sync::RwLock::new(Some(cache))),
        issuer_url: Some("test-issuer".to_string()),
        client_id: Some("test-client".to_string()),
        pepper: b"test".to_vec(),
        cookie_key: cookie::Key::generate(),
        default_user_id: Uuid::nil(),
    };
    let state = make_app_state(client, user_id, None);
    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let token = make_jwt(&key, &kid, "test-subject");
    let resp = reqwest::Client::new()
        .get(format!("http://{}/api/auth/me", addr))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["username"], "testuser");
    assert!(body.get("api_key_hash").is_none());
}

#[tokio::test]
async fn test_generate_api_key() {
    let (client, _tmp) = make_client().await;
    let (key, kid, cache) = make_jwt_cache();
    let user_id = Uuid::new_v4();
    let cookie_key = cookie::Key::from(&[0u8; 64]);
    insert_user(
        &client,
        user_id,
        "apikey-user",
        Some("apikey-sub"),
        false,
        true,
    )
    .await;
    let auth_layer = AuthLayer {
        enabled: true,
        db: client.clone(),
        jwks_cache: Arc::new(tokio::sync::RwLock::new(Some(cache))),
        issuer_url: Some("test-issuer".to_string()),
        client_id: Some("test-client".to_string()),
        pepper: b"test_pepper".to_vec(),
        cookie_key: cookie::Key::generate(),
        default_user_id: Uuid::nil(),
    };
    let state = make_app_state(client.clone(), user_id, None);
    // Use deterministic cookie_key matching the pepper
    let state = AppState {
        cfg_port: 0,

        cookie_key,
        ..state
    };
    let api_key_pepper = state.api_key_pepper.clone();
    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let token = make_jwt(&key, &kid, "apikey-sub");
    let resp = reqwest::Client::new()
        .post(format!("http://{}/api/auth/api-key", addr))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    let api_key = body["api_key"].as_str().unwrap();
    assert!(api_key.starts_with("ccui_"));
    assert_eq!(api_key.len(), 69);

    let hash = api_key::hash_api_key(api_key, &api_key_pepper);
    let mut rows = client
        .query_raw(
            "SELECT api_key_hash FROM users WHERE id = $1",
            params!(user_id.to_string()),
        )
        .await
        .unwrap();
    let stored_hash: Option<String> = rows[0].get("api_key_hash");
    assert_eq!(stored_hash, Some(hash));
}

#[tokio::test]
async fn test_revoke_api_key() {
    let (client, _tmp) = make_client().await;
    let (key, kid, cache) = make_jwt_cache();
    let user_id = Uuid::new_v4();
    insert_user(
        &client,
        user_id,
        "revoke-user",
        Some("revoke-sub"),
        false,
        true,
    )
    .await;
    let existing_hash = api_key::hash_api_key("ccui_some-old-key-12345", b"test");
    client
        .execute(
            "UPDATE users SET api_key_hash = $1 WHERE id = $2",
            params!(existing_hash, user_id.to_string()),
        )
        .await
        .unwrap();

    let auth_layer = AuthLayer {
        enabled: true,
        db: client.clone(),
        jwks_cache: Arc::new(tokio::sync::RwLock::new(Some(cache))),
        issuer_url: Some("test-issuer".to_string()),
        client_id: Some("test-client".to_string()),
        pepper: b"test".to_vec(),
        cookie_key: cookie::Key::generate(),
        default_user_id: Uuid::nil(),
    };
    let state = make_app_state(client.clone(), user_id, None);
    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let token = make_jwt(&key, &kid, "revoke-sub");
    let resp = reqwest::Client::new()
        .delete(format!("http://{}/api/auth/api-key", addr))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let mut rows = client
        .query_raw(
            "SELECT api_key_hash FROM users WHERE id = $1",
            params!(user_id.to_string()),
        )
        .await
        .unwrap();
    let stored_hash: Option<String> = rows[0].get("api_key_hash");
    assert!(stored_hash.is_none());
}

#[tokio::test]
async fn test_api_key_auth_accesses_protected_route() {
    let (client, _tmp) = make_client().await;
    let (_key, _kid, cache) = make_jwt_cache();
    let user_id = Uuid::new_v4();
    let api_key_val = "ccui_test-api-key-for-me-endpoint";
    let hash = api_key::hash_api_key(api_key_val, b"test");
    insert_user(&client, user_id, "apikey-me-user", None, false, true).await;
    client
        .execute(
            "UPDATE users SET api_key_hash = $1 WHERE id = $2",
            params!(hash, user_id.to_string()),
        )
        .await
        .unwrap();

    let auth_layer = AuthLayer {
        enabled: true,
        db: client.clone(),
        jwks_cache: Arc::new(tokio::sync::RwLock::new(Some(cache))),
        issuer_url: Some("test-issuer".to_string()),
        client_id: Some("test-client".to_string()),
        pepper: b"test".to_vec(),
        cookie_key: cookie::Key::generate(),
        default_user_id: Uuid::nil(),
    };
    let state = make_app_state(client, user_id, None);
    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let resp = reqwest::Client::new()
        .get(format!("http://{}/api/auth/me", addr))
        .header("Authorization", format!("Bearer {api_key_val}"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["username"], "apikey-me-user");
}

#[tokio::test]
async fn test_admin_list_users() {
    let (client, _tmp) = make_client().await;
    let (key, kid, cache) = make_jwt_cache();
    let admin_user_id = Uuid::new_v4();
    insert_user(
        &client,
        admin_user_id,
        "admin-user",
        Some("admin-sub"),
        true,
        true,
    )
    .await;
    insert_user(
        &client,
        Uuid::new_v4(),
        "regular-user",
        Some("regular-sub"),
        false,
        true,
    )
    .await;

    let auth_layer = AuthLayer {
        enabled: true,
        db: client.clone(),
        jwks_cache: Arc::new(tokio::sync::RwLock::new(Some(cache))),
        issuer_url: Some("test-issuer".to_string()),
        client_id: Some("test-client".to_string()),
        pepper: b"test".to_vec(),
        cookie_key: cookie::Key::generate(),
        default_user_id: Uuid::nil(),
    };
    let state = make_app_state(client, admin_user_id, None);
    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let token = make_jwt(&key, &kid, "admin-sub");
    let resp = reqwest::Client::new()
        .get(format!("http://{}/api/admin/users", addr))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    let users = body["users"].as_array().unwrap();
    assert!(users.len() >= 2);
}

#[tokio::test]
async fn test_admin_list_denied_for_non_admin() {
    let (client, _tmp) = make_client().await;
    let (key, kid, cache) = make_jwt_cache();
    let user_id = Uuid::new_v4();
    insert_user(
        &client,
        user_id,
        "regular-user",
        Some("nonadmin-sub"),
        false,
        true,
    )
    .await;

    let auth_layer = AuthLayer {
        enabled: true,
        db: client.clone(),
        jwks_cache: Arc::new(tokio::sync::RwLock::new(Some(cache))),
        issuer_url: Some("test-issuer".to_string()),
        client_id: Some("test-client".to_string()),
        pepper: b"test".to_vec(),
        cookie_key: cookie::Key::generate(),
        default_user_id: Uuid::nil(),
    };
    let state = make_app_state(client, user_id, None);
    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let token = make_jwt(&key, &kid, "nonadmin-sub");
    let resp = reqwest::Client::new()
        .get(format!("http://{}/api/admin/users", addr))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 403);
}

#[tokio::test]
async fn test_admin_update_user() {
    let (client, _tmp) = make_client().await;
    let (key, kid, cache) = make_jwt_cache();
    let admin_user_id = Uuid::new_v4();
    let target_user_id = Uuid::new_v4();
    insert_user(
        &client,
        admin_user_id,
        "admin",
        Some("admin-update-sub"),
        true,
        true,
    )
    .await;
    insert_user(
        &client,
        target_user_id,
        "target",
        Some("target-sub"),
        false,
        true,
    )
    .await;

    let auth_layer = AuthLayer {
        enabled: true,
        db: client.clone(),
        jwks_cache: Arc::new(tokio::sync::RwLock::new(Some(cache))),
        issuer_url: Some("test-issuer".to_string()),
        client_id: Some("test-client".to_string()),
        pepper: b"test".to_vec(),
        cookie_key: cookie::Key::generate(),
        default_user_id: Uuid::nil(),
    };
    let state = make_app_state(client.clone(), admin_user_id, None);
    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let token = make_jwt(&key, &kid, "admin-update-sub");
    let resp = reqwest::Client::new()
        .put(format!(
            "http://{}/api/admin/users/{}",
            addr, target_user_id
        ))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({ "is_admin": true, "is_active": false }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let mut rows = client
        .query_raw(
            "SELECT is_admin, is_active FROM users WHERE id = $1",
            params!(target_user_id.to_string()),
        )
        .await
        .unwrap();
    assert_eq!(rows[0].get::<i64>("is_admin"), 1);
    assert_eq!(rows[0].get::<i64>("is_active"), 0);
}

#[tokio::test]
async fn test_admin_cannot_self_demote() {
    let (client, _tmp) = make_client().await;
    let (key, kid, cache) = make_jwt_cache();
    let admin_user_id = Uuid::new_v4();
    insert_user(
        &client,
        admin_user_id,
        "self-demote-admin",
        Some("selfdemote-sub"),
        true,
        true,
    )
    .await;

    let auth_layer = AuthLayer {
        enabled: true,
        db: client.clone(),
        jwks_cache: Arc::new(tokio::sync::RwLock::new(Some(cache))),
        issuer_url: Some("test-issuer".to_string()),
        client_id: Some("test-client".to_string()),
        pepper: b"test".to_vec(),
        cookie_key: cookie::Key::generate(),
        default_user_id: Uuid::nil(),
    };
    let state = make_app_state(client, admin_user_id, None);
    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let token = make_jwt(&key, &kid, "selfdemote-sub");
    let resp = reqwest::Client::new()
        .put(format!("http://{}/api/admin/users/{}", addr, admin_user_id))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({ "is_admin": false }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 403);
}

#[tokio::test]
async fn test_logout_without_cookie_returns_success() {
    let (client, _tmp) = make_client().await;
    let user_id = db::ensure_default_user(&client).await.unwrap();
    let oidc = OidcEndpoints {
        end_session_endpoint: None,
        authorization_endpoint: "https://provider.test/auth".into(),
        token_endpoint: "https://provider.test/token".into(),
        revocation_endpoint: None,
        client_id: "test-client".into(),
        client_secret: None,
        redirect_uri: "http://localhost:3183/api/auth/callback".into(),
        jwks_cache: None,
        jwks_issuer: None,
    };
    let state = make_app_state(client, user_id, Some(oidc));
    let auth_layer = AuthLayer::disabled(
        state.db.clone(),
        b"test".to_vec(),
        state.cookie_key.clone(),
        state.default_user_id,
    );
    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let resp = reqwest::Client::new()
        .post(format!("http://{}/api/auth/logout", addr))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["redirect_url"], serde_json::Value::Null);
}

#[tokio::test]
async fn test_me_returns_unauthorized_when_no_user_matches_jwt() {
    let (client, _tmp) = make_client().await;
    let (key, kid, cache) = make_jwt_cache();
    let user_id = Uuid::new_v4();
    insert_user(&client, user_id, "some-user", Some("some-sub"), false, true).await;

    let auth_layer = AuthLayer {
        enabled: true,
        db: client.clone(),
        jwks_cache: Arc::new(tokio::sync::RwLock::new(Some(cache))),
        issuer_url: Some("test-issuer".to_string()),
        client_id: Some("test-client".to_string()),
        pepper: b"test".to_vec(),
        cookie_key: cookie::Key::generate(),
        default_user_id: Uuid::nil(),
    };
    let state = make_app_state(client, user_id, None);
    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let token = make_jwt(&key, &kid, "nonexistent-subject");
    let resp = reqwest::Client::new()
        .get(format!("http://{}/api/auth/me", addr))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 401);
}

fn make_encrypted_cookie(key: &cookie::Key, name: &str, value: &str) -> String {
    let mut jar = cookie::CookieJar::new();
    {
        let mut private = jar.private_mut(key);
        private.add(cookie::Cookie::new(name.to_owned(), value.to_owned()));
    }
    let c = jar.get(name).unwrap();
    format!("{}={}", c.name(), c.value())
}

#[tokio::test]
async fn test_callback_exchanges_code() {
    let (client, _tmp) = make_client().await;
    let user_id = db::ensure_default_user(&client).await.unwrap();

    let (key, kid, cache) = make_jwt_cache();
    let mut header = Header::new(jsonwebtoken::Algorithm::HS256);
    header.kid = Some(kid);
    let id_token_claims = json!({
        "sub": "callback-sub",
        "preferred_username": "callbackuser",
        "iss": "test-issuer",
        "aud": "test-client",
        "exp": 9_999_999_999_i64,
        "iat": 1_000_000_000,
    });
    let id_token = encode(&header, &id_token_claims, &EncodingKey::from_secret(&key)).unwrap();
    let mock_app = Router::new().route(
        "/token",
        post(move || {
            let id_token = id_token.clone();
            async move {
                Json(json!({
                    "access_token": "mock-access-token",
                    "refresh_token": "mock-refresh-token",
                    "id_token": id_token,
                    "expires_in": 3600
                }))
            }
        }),
    );
    let mock_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mock_addr = mock_listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(mock_listener, mock_app).await.unwrap() });

    let oidc = OidcEndpoints {
        end_session_endpoint: None,
        authorization_endpoint: "https://provider.test/auth".into(),
        token_endpoint: format!("http://{}/token", mock_addr),
        revocation_endpoint: None,
        client_id: "test-client".into(),
        client_secret: None,
        redirect_uri: "http://localhost:3183/api/auth/callback".into(),
        jwks_cache: Some(Arc::new(tokio::sync::RwLock::new(Some(cache)))),
        jwks_issuer: Some("test-issuer".into()),
    };
    let state = make_app_state(client.clone(), user_id, Some(oidc));
    let auth_layer = AuthLayer::disabled(
        state.db.clone(),
        b"test".to_vec(),
        state.cookie_key.clone(),
        state.default_user_id,
    );
    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    // Step 1: Login to populate PKCE store
    let login_resp = reqwest::Client::new()
        .get(format!("http://{}/api/auth/login", addr))
        .send()
        .await
        .unwrap();
    assert_eq!(login_resp.status(), 200);
    let login_body: Value = login_resp.json().await.unwrap();
    let auth_url = login_body["authorization_url"].as_str().unwrap();
    let state_param = auth_url
        .split("state=")
        .nth(1)
        .unwrap()
        .split('&')
        .next()
        .unwrap();

    // Step 2: Callback with code + state
    let http_client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    let resp = http_client
        .get(format!(
            "http://{}/api/auth/callback?code=test_code&state={}",
            addr, state_param
        ))
        .send()
        .await
        .unwrap();
    let status = resp.status();
    let headers = resp.headers().clone();
    let body_text = resp.text().await.unwrap_or_default();
    assert_eq!(status, 302, "expected 302, got {status}: {body_text}");
    let set_cookie = headers
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(
        set_cookie.contains("ofm_session"),
        "set-cookie: {set_cookie}"
    );
    assert!(set_cookie.contains("HttpOnly"), "set-cookie: {set_cookie}");
    assert!(
        set_cookie.contains("SameSite=Lax"),
        "set-cookie: {set_cookie}"
    );

    // Verify user was created in DB
    let mut rows = client
        .query_raw(
            "SELECT * FROM users WHERE oidc_subject = $1",
            params!("callback-sub"),
        )
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get::<String>("username"), "callbackuser");
    assert!(rows[0].get::<i64>("is_active") == 1);
}

#[tokio::test]
async fn test_refresh_with_session_cookie() {
    let (client, _tmp) = make_client().await;
    let default_user_id = db::ensure_default_user(&client).await.unwrap();
    let user_id = Uuid::new_v4();
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    client
        .execute(
            "INSERT INTO users (id, username, oidc_subject, is_active, created_at, has_completed_onboarding, is_technical) VALUES ($1, $2, $3, 1, $4, 1, 0)",
            params!(user_id.to_string(), "refreshuser", "refresh-sub", now.clone()),
        )
        .await
        .unwrap();

    let session_id = Uuid::new_v4();
    let future = (chrono::Utc::now() + chrono::Duration::days(30))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();
    client
        .execute(
            "INSERT INTO sessions (id, user_id, refresh_token, expires_at, created_at) VALUES ($1, $2, $3, $4, $5)",
            params!(session_id.to_string(), user_id.to_string(), "test-refresh-token", future, now),
        )
        .await
        .unwrap();

    // Mock OIDC endpoint for refresh
    let mock_app = Router::new().route(
        "/token",
        post(|| async {
            Json(json!({
                "access_token": "new-access-token",
                "refresh_token": "new-refresh-token",
                "expires_in": 7200
            }))
        }),
    );
    let mock_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mock_addr = mock_listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(mock_listener, mock_app).await.unwrap() });

    let oidc = OidcEndpoints {
        end_session_endpoint: None,
        authorization_endpoint: format!("http://{}/auth", mock_addr),
        token_endpoint: format!("http://{}/token", mock_addr),
        revocation_endpoint: None,
        client_id: "test-client".into(),
        client_secret: None,
        redirect_uri: format!("http://{}/callback", mock_addr),
        jwks_cache: None,
        jwks_issuer: None,
    };

    let key = cookie::Key::generate();
    let state = AppState {
        cfg_port: 0,

        db: client.clone(),
        default_user_id,
        archive_root: "storage/".into(),
        config_root: "/tmp".into(),
        active_sessions: Arc::new(Mutex::new(HashMap::<String, Box<dyn LlmProvider>>::new())),
        oidc_provider: Some(oidc),
        pkce_store: Arc::new(Mutex::new(HashMap::new())),
        cookie_key: key.clone(),
        api_key_pepper: b"test_pepper".to_vec(),
        ws_bus: BroadcastBus::new(),
    };
    let auth_layer = AuthLayer::disabled(
        state.db.clone(),
        b"test".to_vec(),
        state.cookie_key.clone(),
        state.default_user_id,
    );
    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let cookie_str = make_encrypted_cookie(&key, "ofm_session", &session_id.to_string());
    let resp = reqwest::Client::new()
        .post(format!("http://{}/api/auth/refresh", addr))
        .header("Cookie", cookie_str)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["access_token"], "new-access-token");
}

#[tokio::test]
async fn test_refresh_without_cookie() {
    let (client, _tmp) = make_client().await;
    let user_id = db::ensure_default_user(&client).await.unwrap();
    let oidc = OidcEndpoints {
        end_session_endpoint: None,
        authorization_endpoint: "https://provider.test/auth".into(),
        token_endpoint: "https://provider.test/token".into(),
        revocation_endpoint: None,
        client_id: "test-client".into(),
        client_secret: None,
        redirect_uri: "http://localhost:3183/api/auth/callback".into(),
        jwks_cache: None,
        jwks_issuer: None,
    };
    let state = make_app_state(client, user_id, Some(oidc));
    let auth_layer = AuthLayer::disabled(
        state.db.clone(),
        b"test".to_vec(),
        state.cookie_key.clone(),
        state.default_user_id,
    );
    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let resp = reqwest::Client::new()
        .post(format!("http://{}/api/auth/refresh", addr))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn test_onboarding_with_valid_auth() {
    let (client, _tmp) = make_client().await;
    let user_id = Uuid::new_v4();
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    client
        .execute(
            "INSERT INTO users (id, username, oidc_subject, is_admin, is_active, created_at, has_completed_onboarding, is_technical) VALUES ($1, $2, $3, 0, 1, $4, 0, 0)",
            params!(user_id.to_string(), "onboarduser", "onboard-sub", now),
        )
        .await
        .unwrap();

    let (key, kid, cache) = make_jwt_cache();
    let token = make_jwt(&key, &kid, "onboard-sub");

    let jwks_cache = Arc::new(tokio::sync::RwLock::new(Some(cache)));
    let auth_layer = AuthLayer {
        enabled: true,
        db: client.clone(),
        jwks_cache,
        issuer_url: Some("test-issuer".to_string()),
        client_id: Some("test-client".to_string()),
        pepper: b"test".to_vec(),
        cookie_key: cookie::Key::generate(),
        default_user_id: Uuid::nil(),
    };
    let state = make_app_state(client, user_id, None);
    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let resp = reqwest::Client::new()
        .patch(format!("http://{}/api/auth/onboarding", addr))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({
            "git_name": "Jane Doe",
            "git_email": "jane@example.com",
            "is_technical": true,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["success"], true);
    assert_eq!(body["user"]["git_name"], "Jane Doe");
    assert_eq!(body["user"]["git_email"], "jane@example.com");
    assert_eq!(body["user"]["is_technical"], true);
    assert_eq!(body["user"]["has_completed_onboarding"], true);
}

#[tokio::test]
async fn test_onboarding_with_missing_fields_returns_400() {
    let (client, _tmp) = make_client().await;
    let user_id = Uuid::new_v4();
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    client
        .execute(
            "INSERT INTO users (id, username, oidc_subject, is_admin, is_active, created_at, has_completed_onboarding, is_technical) VALUES ($1, $2, $3, 0, 1, $4, 0, 0)",
            params!(user_id.to_string(), "missingfields", "missing-sub", now),
        )
        .await
        .unwrap();

    let (key, kid, cache) = make_jwt_cache();
    let token = make_jwt(&key, &kid, "missing-sub");

    let auth_layer = AuthLayer {
        enabled: true,
        db: client.clone(),
        jwks_cache: Arc::new(tokio::sync::RwLock::new(Some(cache))),
        issuer_url: Some("test-issuer".to_string()),
        client_id: Some("test-client".to_string()),
        pepper: b"test".to_vec(),
        cookie_key: cookie::Key::generate(),
        default_user_id: Uuid::nil(),
    };
    let state = make_app_state(client, user_id, None);
    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    // Send empty body
    let resp = reqwest::Client::new()
        .patch(format!("http://{}/api/auth/onboarding", addr))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 422, "expected 422 for empty payload");

    // Send partial payload (missing is_technical - required field)
    let resp = reqwest::Client::new()
        .patch(format!("http://{}/api/auth/onboarding", addr))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({
            "git_name": "Partial",
            "git_email": "partial@example.com",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 422);
}

#[tokio::test]
async fn test_onboarding_db_round_trip() {
    let (client, _tmp) = make_client().await;
    let user_id = Uuid::new_v4();
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

    client
        .execute(
            "INSERT INTO users (id, username, oidc_subject, is_active, created_at) VALUES ($1, $2, $3, 1, $4)",
            params!(user_id.to_string(), "roundtrip", "roundtrip-sub", now),
        )
        .await
        .unwrap();

    let user = current_user(&client, user_id).await.unwrap();
    assert!(!user.has_completed_onboarding);
    assert_eq!(user.git_name, None);
    assert_eq!(user.git_email, None);

    let saved = complete_onboarding(
        &client,
        user_id,
        "Jane Doe".into(),
        "jane@example.com".into(),
        true,
    )
    .await
    .unwrap();

    assert!(saved.has_completed_onboarding);
    assert_eq!(saved.git_name, Some("Jane Doe".into()));
    assert_eq!(saved.git_email, Some("jane@example.com".into()));
    assert!(saved.is_technical);

    let fetched = current_user(&client, user_id).await.unwrap();
    assert!(fetched.has_completed_onboarding);
    assert_eq!(fetched.git_name, Some("Jane Doe".into()));
    assert_eq!(fetched.git_email, Some("jane@example.com".into()));
    assert!(fetched.is_technical);
}

#[tokio::test]
async fn test_onboarding_without_auth_returns_401() {
    let (client, _tmp) = make_client().await;
    let user_id = db::ensure_default_user(&client).await.unwrap();
    let auth_layer = AuthLayer {
        enabled: true,
        db: client.clone(),
        jwks_cache: Arc::new(tokio::sync::RwLock::new(None)),
        issuer_url: Some("test-issuer".to_string()),
        client_id: Some("test-client".to_string()),
        pepper: b"test".to_vec(),
        cookie_key: cookie::Key::generate(),
        default_user_id: Uuid::nil(),
    };
    let state = make_app_state(client, user_id, None);
    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let resp = reqwest::Client::new()
        .patch(format!("http://{}/api/auth/onboarding", addr))
        .json(&json!({
            "git_name": "No Auth",
            "git_email": "noauth@example.com",
            "is_technical": false,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}
