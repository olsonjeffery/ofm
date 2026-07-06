use std::collections::HashMap;
use std::sync::Arc;

use omprint::auth::api_key;
use omprint::auth::AuthLayer;
use omprint::db;
use omprint::providers::LlmProvider;
use omprint::server;
use omprint::server::state::AppState;
use tokio::sync::Mutex;

fn make_api_key() -> (String, String) {
    let key = "ccui_settings_test_api_key_v1";
    let hash = api_key::hash_api_key(key, b"test_pepper_16");
    (key.to_string(), hash)
}

async fn make_state_with_auth() -> (AppState, AuthLayer, String, tempfile::TempDir) {
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
    let user_id = db::ensure_default_user(&client).await.unwrap();

    let (api_key_str, hash) = make_api_key();
    client
        .execute(
            "UPDATE users SET api_key_hash = $1 WHERE id = $2",
            hiqlite::params!(hash, user_id.to_string()),
        )
        .await
        .unwrap();

    let auth_layer = AuthLayer {
        enabled: true,
        db: client.clone(),
        jwks_cache: Arc::new(tokio::sync::RwLock::new(None)),
        issuer_url: None,
        client_id: None,
        pepper: b"test_pepper_16".to_vec(),
    };

    let state = AppState {
        db: client,
        default_user_id: user_id,
        archive_root: "storage/".into(),
        config_root: tmp.path().to_str().unwrap().to_string(),
        omp_sessions: Arc::new(Mutex::new(HashMap::new())),
        active_sessions: Arc::new(Mutex::new(HashMap::<String, Box<dyn LlmProvider>>::new())),
        oidc_provider: None,
        pkce_store: Arc::new(Mutex::new(HashMap::new())),
        cookie_key: cookie::Key::generate(),
    };

    (state, auth_layer, api_key_str, tmp)
}

async fn make_state_no_auth() -> (AppState, AuthLayer, tempfile::TempDir) {
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
    let user_id = db::ensure_default_user(&client).await.unwrap();
    let auth_layer = AuthLayer::disabled(client.clone(), b"test".to_vec());
    let state = AppState {
        db: client,
        default_user_id: user_id,
        archive_root: "storage/".into(),
        config_root: tmp.path().to_str().unwrap().to_string(),
        omp_sessions: Arc::new(Mutex::new(HashMap::new())),
        active_sessions: Arc::new(Mutex::new(HashMap::<String, Box<dyn LlmProvider>>::new())),
        oidc_provider: None,
        pkce_store: Arc::new(Mutex::new(HashMap::new())),
        cookie_key: cookie::Key::generate(),
    };
    (state, auth_layer, tmp)
}

async fn spawn_app(state: AppState, auth_layer: AuthLayer) -> String {
    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{}", addr)
}

async fn create_config(
    base_url: &str,
    api_key: &str,
    name: &str,
    config_body: &str,
    harness: &str,
) -> reqwest::Response {
    let client = reqwest::Client::new();
    client
        .post(format!("{base_url}/api/settings/config-body"))
        .header("Authorization", format!("Bearer {api_key}"))
        .json(&serde_json::json!({
            "name": name,
            "config_body": config_body,
            "harness": harness,
        }))
        .send()
        .await
        .unwrap()
}

async fn list_configs(base_url: &str, api_key: &str) -> reqwest::Response {
    let client = reqwest::Client::new();
    client
        .get(format!("{base_url}/api/settings/config-body"))
        .header("Authorization", format!("Bearer {api_key}"))
        .send()
        .await
        .unwrap()
}

async fn update_config(
    base_url: &str,
    api_key: &str,
    id: &str,
    name: &str,
    config_body: &str,
    harness: &str,
) -> reqwest::Response {
    let client = reqwest::Client::new();
    client
        .put(format!("{base_url}/api/settings/config-body/{id}"))
        .header("Authorization", format!("Bearer {api_key}"))
        .json(&serde_json::json!({
            "name": name,
            "config_body": config_body,
            "harness": harness,
        }))
        .send()
        .await
        .unwrap()
}

async fn delete_config(base_url: &str, api_key: &str, id: &str) -> reqwest::Response {
    let client = reqwest::Client::new();
    client
        .delete(format!("{base_url}/api/settings/config-body/{id}"))
        .header("Authorization", format!("Bearer {api_key}"))
        .send()
        .await
        .unwrap()
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_settings_config_body_crud() {
    let (state, auth_layer, api_key, _tmp) = make_state_with_auth().await;
    let base_url = spawn_app(state, auth_layer).await;

    // Create
    let resp = create_config(&base_url, &api_key, "test-cfg", "key: value", "openai").await;
    assert_eq!(resp.status(), 201);
    let body: serde_json::Value = resp.json().await.unwrap();
    let id = body["id"].as_str().unwrap().to_string();
    assert_eq!(body["name"], "test-cfg");
    assert_eq!(body["config_body"], "key: value");
    assert_eq!(body["harness"], "openai");

    // List (verify created entry)
    let resp = list_configs(&base_url, &api_key).await;
    assert_eq!(resp.status(), 200);
    let list: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["id"], id);

    // Update
    let resp = update_config(
        &base_url,
        &api_key,
        &id,
        "updated-cfg",
        r#"{"key": "updated"}"#,
        "anthropic",
    )
    .await;
    assert_eq!(resp.status(), 200);
    let updated: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(updated["name"], "updated-cfg");
    assert_eq!(updated["config_body"], r#"{"key": "updated"}"#);
    assert_eq!(updated["harness"], "anthropic");

    // List (verify update persisted)
    let resp = list_configs(&base_url, &api_key).await;
    assert_eq!(resp.status(), 200);
    let list: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["name"], "updated-cfg");

    // Delete
    let resp = delete_config(&base_url, &api_key, &id).await;
    assert_eq!(resp.status(), 204);

    // List (verify empty)
    let resp = list_configs(&base_url, &api_key).await;
    assert_eq!(resp.status(), 200);
    let list: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert!(list.is_empty());
}

#[tokio::test]
async fn test_settings_config_body_not_found() {
    let (state, auth_layer, api_key, _tmp) = make_state_with_auth().await;
    let base_url = spawn_app(state, auth_layer).await;

    let fake_id = uuid::Uuid::new_v4().to_string();

    // Update non-existent
    let resp = update_config(&base_url, &api_key, &fake_id, "nope", "key: val", "").await;
    assert_eq!(resp.status(), 404);

    // Delete non-existent
    let resp = delete_config(&base_url, &api_key, &fake_id).await;
    assert_eq!(resp.status(), 204); // DELETE is idempotent
}

#[tokio::test]
async fn test_settings_config_body_user_isolation() {
    // Use a single DB with two users to test user-level isolation
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
    let user_a_id = db::ensure_default_user(&client).await.unwrap();

    // User A API key
    let (user_a_key, user_a_hash) = {
        let key = "ccui_user_a_isolation_key";
        (key.to_string(), api_key::hash_api_key(key, b"test_pepper_16"))
    };
    client
        .execute(
            "UPDATE users SET api_key_hash = $1 WHERE id = $2",
            hiqlite::params!(user_a_hash, user_a_id.to_string()),
        )
        .await
        .unwrap();

    // User B
    let user_b_id = uuid::Uuid::new_v4();
    let (user_b_key, user_b_hash) = {
        let key = "ccui_user_b_isolation_key";
        (key.to_string(), api_key::hash_api_key(key, b"test_pepper_16"))
    };
    client
        .execute(
            "INSERT INTO users (id, username, api_key_hash, is_admin, is_technical, has_completed_onboarding, token_version) VALUES ($1, $2, $3, $4, $5, $6, $7)",
            hiqlite::params!(
                user_b_id.to_string(),
                "user-b",
                user_b_hash,
                0i64, 0i64, 1i64, 0i64
            ),
        )
        .await
        .unwrap();

    let auth_layer = AuthLayer {
        enabled: true,
        db: client.clone(),
        jwks_cache: Arc::new(tokio::sync::RwLock::new(None)),
        issuer_url: None,
        client_id: None,
        pepper: b"test_pepper_16".to_vec(),
    };

    let state = AppState {
        db: client,
        default_user_id: user_a_id,
        archive_root: "storage/".into(),
        config_root: tmp.path().to_str().unwrap().to_string(),
        omp_sessions: Arc::new(Mutex::new(HashMap::new())),
        active_sessions: Arc::new(Mutex::new(HashMap::<String, Box<dyn LlmProvider>>::new())),
        oidc_provider: None,
        pkce_store: Arc::new(Mutex::new(HashMap::new())),
        cookie_key: cookie::Key::generate(),
    };

    let base_url = spawn_app(state, auth_layer).await;

    // User A creates config
    let resp = create_config(&base_url, &user_a_key, "user-a-cfg", "key: value", "openai").await;
    assert_eq!(resp.status(), 201);
    let created: serde_json::Value = resp.json().await.unwrap();
    let config_id = created["id"].as_str().unwrap().to_string();

    // User B lists → empty (user isolation)
    let resp = list_configs(&base_url, &user_b_key).await;
    assert_eq!(resp.status(), 200);
    let list: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert!(list.is_empty());

    // User B tries to delete User A's config → 204 (no match due to user_id filter)
    let resp = delete_config(&base_url, &user_b_key, &config_id).await;
    assert_eq!(resp.status(), 204);

    // Verify config still exists for User A
    let resp = list_configs(&base_url, &user_a_key).await;
    assert_eq!(resp.status(), 200);
    let list: Vec<serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["id"], config_id);
}

#[tokio::test]
async fn test_settings_agent_models_rw() {
    let (state, auth_layer, api_key, _tmp) = make_state_with_auth().await;
    let base_url = spawn_app(state, auth_layer).await;

    let client = reqwest::Client::new();

    // GET initial → empty
    let resp = client
        .get(format!("{base_url}/api/settings/agent-models"))
        .header("Authorization", format!("Bearer {api_key}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let initial: HashMap<String, serde_json::Value> = resp.json().await.unwrap();
    assert!(initial.is_empty());

    // PUT three agent types
    let models = serde_json::json!({
        "planification": {"model": "gpt-4", "effort": "high"},
        "implementation": {"model": "gpt-4o", "effort": "medium"},
        "review": {"model": "claude-3", "effort": "auto"},
    });
    let resp = client
        .put(format!("{base_url}/api/settings/agent-models"))
        .header("Authorization", format!("Bearer {api_key}"))
        .json(&models)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let saved: HashMap<String, serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(saved.len(), 3);
    assert_eq!(saved["planification"]["model"], "gpt-4");
    assert_eq!(saved["implementation"]["effort"], "medium");

    // GET after PUT
    let resp = client
        .get(format!("{base_url}/api/settings/agent-models"))
        .header("Authorization", format!("Bearer {api_key}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let fetched: HashMap<String, serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(fetched.len(), 3);
    assert_eq!(fetched["review"]["model"], "claude-3");

    // PUT partial update
    let partial = serde_json::json!({
        "implementation": {"model": "gpt-4-turbo", "effort": "low"},
    });
    let resp = client
        .put(format!("{base_url}/api/settings/agent-models"))
        .header("Authorization", format!("Bearer {api_key}"))
        .json(&partial)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let after_partial: HashMap<String, serde_json::Value> = resp.json().await.unwrap();
    assert_eq!(after_partial["implementation"]["model"], "gpt-4-turbo");
    assert_eq!(after_partial["implementation"]["effort"], "low");
    // Other agents should still be present
    assert!(after_partial.contains_key("planification"));
    assert!(after_partial.contains_key("review"));
}

#[tokio::test]
async fn test_settings_requires_auth() {
    let (state, auth_layer, _api_key, _tmp) = make_state_with_auth().await;
    let base_url = spawn_app(state, auth_layer).await;

    let client = reqwest::Client::new();

    // GET config-body without auth
    let resp = client
        .get(format!("{base_url}/api/settings/config-body"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    // POST config-body without auth
    let resp = client
        .post(format!("{base_url}/api/settings/config-body"))
        .json(&serde_json::json!({"name": "x", "config_body": "key: val"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    // PUT config-body without auth
    let resp = client
        .put(format!("{base_url}/api/settings/config-body/{}", uuid::Uuid::new_v4()))
        .json(&serde_json::json!({"name": "x", "config_body": "key: val"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    // DELETE config-body without auth
    let resp = client
        .delete(format!("{base_url}/api/settings/config-body/{}", uuid::Uuid::new_v4()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    // GET agent-models without auth
    let resp = client
        .get(format!("{base_url}/api/settings/agent-models"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);

    // PUT agent-models without auth
    let resp = client
        .put(format!("{base_url}/api/settings/agent-models"))
        .json(&serde_json::json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn test_settings_page_renders() {
    let (state, auth_layer, _tmp) = make_state_no_auth().await;
    let base_url = spawn_app(state, auth_layer).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{base_url}/webapp/settings"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(body.contains("Settings"));
    assert!(body.contains("Model Configurations"));
    assert!(body.contains("Agent Settings"));
    assert!(body.contains("API Keys"));
}

#[tokio::test]
async fn test_settings_config_body_with_json_body() {
    let (state, auth_layer, api_key, _tmp) = make_state_with_auth().await;
    let base_url = spawn_app(state, auth_layer).await;

    let json_body = r#"{"model": "gpt-4", "temperature": 0.7, "top_p": 1}"#;
    let resp = create_config(&base_url, &api_key, "json-config", json_body, "openai").await;
    assert_eq!(resp.status(), 201);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["config_body"], json_body);
    assert_eq!(body["name"], "json-config");
}

#[tokio::test]
async fn test_settings_config_body_invalid_body_rejected() {
    let (state, auth_layer, api_key, _tmp) = make_state_with_auth().await;
    let base_url = spawn_app(state, auth_layer).await;

    let resp = create_config(&base_url, &api_key, "bad-config", "{{{not valid", "").await;
    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn test_config_format_normalization() {
    // Unit-test-level assertions via the service module
    use omprint::services::config_format;

    // YAML in → to_yaml returns as-is
    let yaml = "model: gpt-4\ntemperature: 0.7\n";
    let result = config_format::to_yaml(yaml).unwrap();
    assert_eq!(result, yaml);

    // YAML in → to_json converts
    let result = config_format::to_json(yaml).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(parsed["model"], "gpt-4");
    assert_eq!(parsed["temperature"], 0.7);

    // JSON in → to_json returns as-is
    let json = r#"{"model":"gpt-4","temperature":0.7}"#;
    let result = config_format::to_json(json).unwrap();
    assert_eq!(result, json);

    // JSON in → to_yaml converts
    let result = config_format::to_yaml(json).unwrap();
    assert!(result.contains("model: gpt-4"));
    assert!(result.contains("temperature: 0.7"));

    // Invalid input → validate fails
    assert!(config_format::validate("{{{").is_err());
}

#[tokio::test]
async fn test_config_format_detect() {
    use omprint::services::config_format;

    // JSON detected
    assert_eq!(
        config_format::detect_format(r#"{"a":1}"#),
        Some(config_format::ConfigFormat::Json)
    );

    // YAML detected
    assert_eq!(
        config_format::detect_format("a: 1\nb: 2\n"),
        Some(config_format::ConfigFormat::Yaml)
    );

    // Invalid → None
    assert_eq!(config_format::detect_format("{{{"), None);
}
