use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use base64::Engine;
use rand::Rng;
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::auth::api_key;
use crate::db::schema::{SessionDb, User};
use crate::server::error::ServerError;
use crate::server::state::{OidcEndpoints, PkceEntry};

const PKCE_TTL: std::time::Duration = std::time::Duration::from_secs(300);
const SESSION_DURATION: std::time::Duration = std::time::Duration::from_secs(30 * 24 * 3600);

pub fn generate_code_verifier() -> String {
    let bytes: [u8; 32] = rand::thread_rng().gen();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

pub fn compute_code_challenge(verifier: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let hash = hasher.finalize();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hash)
}

pub fn generate_state() -> String {
    let bytes: [u8; 32] = rand::thread_rng().gen();
    hex::encode(bytes)
}

pub fn generate_api_key_value() -> String {
    let bytes: [u8; 32] = rand::thread_rng().gen();
    format!("ccui_{}", hex::encode(bytes))
}

fn sweep_expired_pkce(store: &mut HashMap<String, PkceEntry>) {
    store.retain(|_, entry| entry.created_at.elapsed() <= PKCE_TTL);
}

pub async fn initiate_login(
    oidc: &OidcEndpoints,
    pkce_store: &Arc<Mutex<HashMap<String, PkceEntry>>>,
) -> Result<String, ServerError> {
    let code_verifier = generate_code_verifier();
    let code_challenge = compute_code_challenge(&code_verifier);
    let state = generate_state();

    let entry = PkceEntry {
        code_verifier,
        csrf_state: state.clone(),
        created_at: Instant::now(),
    };

    let mut store = pkce_store.lock().await;
    sweep_expired_pkce(&mut store);
    store.insert(state.clone(), entry);

    let auth_url = format!(
        "{}?response_type=code&client_id={}&redirect_uri={}&code_challenge={}&code_challenge_method=S256&scope=openid+profile+email&state={}",
        oidc.authorization_endpoint,
        urlencoding(&oidc.client_id),
        urlencoding(&oidc.redirect_uri),
        &code_challenge,
        &state,
    );

    Ok(auth_url)
}

#[derive(Debug)]
pub struct CallbackResult {
    pub session_id: Uuid,
    pub new_user: bool,
}

pub async fn handle_callback(
    db: &hiqlite::Client,
    oidc: &OidcEndpoints,
    pkce_store: &Arc<Mutex<HashMap<String, PkceEntry>>>,
    code: String,
    state: String,
) -> Result<CallbackResult, ServerError> {
    let entry = pkce_store
        .lock()
        .await
        .remove(&state)
        .ok_or_else(|| ServerError::BadRequest("invalid or expired state parameter".into()))?;

    if entry.created_at.elapsed() > PKCE_TTL {
        return Err(ServerError::BadRequest("state parameter expired".into()));
    }

    let token_body = format!(
        "grant_type=authorization_code&code={}&client_id={}&redirect_uri={}&code_verifier={}{}",
        urlencoding(&code),
        urlencoding(&oidc.client_id),
        urlencoding(&oidc.redirect_uri),
        urlencoding(&entry.code_verifier),
        oidc.client_secret
            .as_ref()
            .map(|s| format!("&client_secret={}", urlencoding(s)))
            .unwrap_or_default(),
    );

    let client = reqwest::Client::new();
    let token_resp = client
        .post(&oidc.token_endpoint)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(token_body)
        .send()
        .await
        .map_err(|e| ServerError::Internal(format!("token exchange failed: {e}")))?;

    let token_data: serde_json::Value = token_resp
        .json()
        .await
        .map_err(|e| ServerError::Internal(format!("invalid token response: {e}")))?;

    let refresh_token = token_data["refresh_token"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let id_token = token_data["id_token"].as_str().map(|s| s.to_string());
    let expires_in = token_data["expires_in"].as_i64().unwrap_or(3600);

    let (sub, username) = {
        let id_token = id_token.as_ref().ok_or_else(|| {
            ServerError::Internal("No id_token returned from provider; cannot authenticate".into())
        })?;

        let jwks_cache = oidc.jwks_cache.as_ref().ok_or_else(|| {
            ServerError::Internal("JWKS not configured; cannot verify identity token".into())
        })?;
        let cache_guard = jwks_cache.read().await;
        let cache = cache_guard.as_ref().ok_or_else(|| {
            ServerError::Internal("JWKS cache not initialized for id_token verification".into())
        })?;

        let claims = match crate::auth::jwks::verify_token(id_token, cache) {
            Ok(c) => c,
            Err(e) => {
                return Err(ServerError::Internal(format!(
                    "id_token verification failed: {e:?}"
                )));
            }
        };

        let username = {
            let payload = decode_jwt_payload(id_token)
                .map_err(|e| ServerError::Internal(format!("invalid id_token: {e}")))?;
            payload["preferred_username"]
                .as_str()
                .map(|s| s.to_string())
        };

        (claims.sub, username)
    };

    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let expires_at = (chrono::Utc::now() + chrono::Duration::seconds(expires_in as i64))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();

    let existing_user = find_user_by_oidc_sub(db, &sub).await?;
    let user_token_version = existing_user.as_ref().map(|u| u.token_version).unwrap_or(0);
    let (user_id, new_user) = if let Some(user) = existing_user {
        db.execute(
            "UPDATE users SET last_login = $1 WHERE id = $2",
            hiqlite::params!(now.clone(), user.id.to_string()),
        )
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;
        (user.id, false)
    } else {
        let id = Uuid::new_v4();
        let username = username.unwrap_or_else(|| sub.clone());
        db.execute(
            "INSERT INTO users (id, username, oidc_subject, is_active, created_at) VALUES ($1, $2, $3, 1, $4)",
            hiqlite::params!(id.to_string(), username, sub, now.clone()),
        )
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;
        (id, true)
    };

    let session_id = Uuid::new_v4();
    db.execute(
        "INSERT INTO sessions (id, user_id, refresh_token, expires_at, created_at, token_version) VALUES ($1, $2, $3, $4, $5, $6)",
        hiqlite::params!(session_id.to_string(), user_id.to_string(), refresh_token, expires_at, now, user_token_version),
    )
    .await
    .map_err(|e| ServerError::Internal(e.to_string()))?;

    Ok(CallbackResult {
        session_id,
        new_user,
    })
}

pub async fn refresh_access_token(
    db: &hiqlite::Client,
    oidc: &OidcEndpoints,
    session_id: Uuid,
) -> Result<String, ServerError> {
    let session = find_session(db, session_id)
        .await?
        .ok_or_else(|| ServerError::BadRequest("session not found or expired".into()))?;

    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    if session.expires_at < now {
        return Err(ServerError::BadRequest("session expired".into()));
    }

    let refresh_body = format!(
        "grant_type=refresh_token&refresh_token={}&client_id={}{}",
        urlencoding(&session.refresh_token),
        urlencoding(&oidc.client_id),
        oidc.client_secret
            .as_ref()
            .map(|s| format!("&client_secret={}", urlencoding(s)))
            .unwrap_or_default(),
    );

    let client = reqwest::Client::new();
    let resp = client
        .post(&oidc.token_endpoint)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(refresh_body)
        .send()
        .await
        .map_err(|e| ServerError::Internal(format!("refresh failed: {e}")))?;

    let status = resp.status();
    let data: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| ServerError::Internal(format!("invalid refresh response: {e}")))?;

    if !status.is_success() {
        let error = data["error"].as_str().unwrap_or("unknown_error");
        if error == "invalid_grant" || error == "invalid_token" {
            db.execute(
                "DELETE FROM sessions WHERE id = $1",
                hiqlite::params!(session_id.to_string()),
            )
            .await
            .map_err(|e| ServerError::Internal(e.to_string()))?;
        }
        return Err(ServerError::Internal(format!(
            "token refresh rejected: {error}"
        )));
    }

    let new_access_token = data["access_token"]
        .as_str()
        .ok_or_else(|| ServerError::Internal("missing access_token".into()))?
        .to_string();
    let new_refresh_token = data["refresh_token"].as_str().unwrap_or("").to_string();
    let expires_in = data["expires_in"].as_i64().unwrap_or(3600);
    let new_expires_at = (chrono::Utc::now() + chrono::Duration::seconds(expires_in))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();

    db.execute(
        "UPDATE sessions SET refresh_token = $1, expires_at = $2 WHERE id = $3",
        hiqlite::params!(new_refresh_token, new_expires_at, session_id.to_string()),
    )
    .await
    .map_err(|e| ServerError::Internal(e.to_string()))?;

    Ok(new_access_token)
}

pub async fn logout(
    db: &hiqlite::Client,
    oidc: &Option<OidcEndpoints>,
    session_id: Uuid,
) -> Result<(), ServerError> {
    let session = find_session(db, session_id).await?;

    if let Some(session) = session {
        let refresh_token = session.refresh_token.clone();

        db.execute(
            "DELETE FROM sessions WHERE id = $1",
            hiqlite::params!(session_id.to_string()),
        )
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;

        if let Some(oidc) = oidc {
            if let Some(revocation_endpoint) = &oidc.revocation_endpoint {
                let revoke_body = format!(
                    "token={}&token_type_hint=refresh_token{}",
                    urlencoding(&refresh_token),
                    oidc.client_secret
                        .as_ref()
                        .map(|s| format!("&client_secret={}", urlencoding(s)))
                        .unwrap_or_default(),
                );
                let client = reqwest::Client::new();
                let _ = client
                    .post(revocation_endpoint)
                    .header("Content-Type", "application/x-www-form-urlencoded")
                    .body(revoke_body)
                    .send()
                    .await;
            }
        }
    }

    Ok(())
}

pub async fn complete_onboarding(
    db: &hiqlite::Client,
    user_id: Uuid,
    git_name: String,
    git_email: String,
    is_technical: bool,
) -> Result<User, ServerError> {
    let tech = is_technical as i64;
    db.execute(
        "UPDATE users SET git_name = $1, git_email = $2, is_technical = $3, has_completed_onboarding = 1 WHERE id = $4",
        hiqlite::params!(git_name, git_email, tech, user_id.to_string()),
    )
    .await
    .map_err(|e| ServerError::Internal(e.to_string()))?;
    current_user(db, user_id).await
}

pub async fn current_user(db: &hiqlite::Client, user_id: Uuid) -> Result<User, ServerError> {
    let user = get_user_by_id(db, user_id)
        .await?
        .ok_or_else(|| ServerError::NotFound("user not found".into()))?;
    Ok(user)
}

pub async fn generate_api_key(
    db: &hiqlite::Client,
    user_id: Uuid,
    pepper: &[u8],
) -> Result<String, ServerError> {
    let api_key = generate_api_key_value();
    let hash = api_key::hash_api_key(&api_key, pepper);
    db.execute(
        "UPDATE users SET api_key_hash = $1, api_key_last_used_at = NULL WHERE id = $2",
        hiqlite::params!(hash, user_id.to_string()),
    )
    .await
    .map_err(|e| ServerError::Internal(e.to_string()))?;
    Ok(api_key)
}

pub async fn revoke_api_key(db: &hiqlite::Client, user_id: Uuid) -> Result<(), ServerError> {
    db.execute(
        "UPDATE users SET api_key_hash = NULL, api_key_last_used_at = NULL WHERE id = $1",
        hiqlite::params!(user_id.to_string()),
    )
    .await
    .map_err(|e| ServerError::Internal(e.to_string()))?;
    Ok(())
}

pub async fn list_users(db: &hiqlite::Client) -> Result<Vec<User>, ServerError> {
    let mut rows = db
        .query_raw(
            "SELECT * FROM users ORDER BY created_at DESC",
            hiqlite::params!(),
        )
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;
    let users: Vec<User> = rows.iter_mut().map(|row| User::from(&mut *row)).collect();
    Ok(users)
}

pub async fn update_user(
    db: &hiqlite::Client,
    target_id: Uuid,
    is_admin: Option<bool>,
    is_active: Option<bool>,
    invalidate_tokens: Option<bool>,
    current_user: &UpdateUserContext,
) -> Result<User, ServerError> {
    if target_id == current_user.user_id && is_admin == Some(false) {
        return Err(ServerError::Forbidden(
            "cannot remove your own admin role".into(),
        ));
    }

    if let Some(admin) = is_admin {
        db.execute(
            "UPDATE users SET is_admin = $1 WHERE id = $2",
            hiqlite::params!(admin as i64, target_id.to_string()),
        )
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;
    }

    if let Some(active) = is_active {
        db.execute(
            "UPDATE users SET is_active = $1 WHERE id = $2",
            hiqlite::params!(active as i64, target_id.to_string()),
        )
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;
    }

    if invalidate_tokens == Some(true) {
        db.execute(
            "UPDATE users SET token_version = token_version + 1 WHERE id = $1",
            hiqlite::params!(target_id.to_string()),
        )
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;

        db.execute(
            "DELETE FROM sessions WHERE user_id = $1",
            hiqlite::params!(target_id.to_string()),
        )
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;
    }

    get_user_by_id(db, target_id)
        .await?
        .ok_or_else(|| ServerError::NotFound("user not found".into()))
}

// Internal helpers

async fn find_user_by_oidc_sub(
    db: &hiqlite::Client,
    sub: &str,
) -> Result<Option<User>, ServerError> {
    let mut rows = db
        .query_raw(
            "SELECT * FROM users WHERE oidc_subject = $1",
            hiqlite::params!(sub),
        )
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;
    Ok(rows.first_mut().map(|row| User::from(&mut *row)))
}

async fn find_session(
    db: &hiqlite::Client,
    session_id: Uuid,
) -> Result<Option<SessionDb>, ServerError> {
    let mut rows = db
        .query_raw(
            "SELECT * FROM sessions WHERE id = $1",
            hiqlite::params!(session_id.to_string()),
        )
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;
    Ok(rows.first_mut().map(|row| SessionDb::from(&mut *row)))
}

async fn get_user_by_id(db: &hiqlite::Client, user_id: Uuid) -> Result<Option<User>, ServerError> {
    let mut rows = db
        .query_raw(
            "SELECT * FROM users WHERE id = $1",
            hiqlite::params!(user_id.to_string()),
        )
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;
    Ok(rows.first_mut().map(|row| User::from(&mut *row)))
}

fn decode_jwt_payload(token: &str) -> Result<serde_json::Value, String> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() < 2 {
        return Err("not a valid JWT".into());
    }
    let payload_b64 = parts[1];
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload_b64.as_bytes())
        .map_err(|e| format!("base64 decode failed: {e}"))?;
    serde_json::from_slice(&decoded).map_err(|e| format!("json parse failed: {e}"))
}

fn urlencoding(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

pub struct UpdateUserContext {
    pub user_id: Uuid,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_code_verifier_length() {
        let verifier = generate_code_verifier();
        assert_eq!(verifier.len(), 43);
    }

    #[test]
    fn test_code_verifier_url_safe() {
        let verifier = generate_code_verifier();
        for c in verifier.chars() {
            assert!(c.is_ascii_alphanumeric() || c == '-' || c == '_');
        }
    }

    #[test]
    fn test_compute_code_challenge_deterministic() {
        let verifier = "test-verifier-12345";
        let challenge1 = compute_code_challenge(verifier);
        let challenge2 = compute_code_challenge(verifier);
        assert_eq!(challenge1, challenge2);
        assert!(!challenge1.is_empty());
    }

    #[test]
    fn test_generate_state_length() {
        let state = generate_state();
        assert_eq!(state.len(), 64);
    }

    #[test]
    fn test_generate_state_hex() {
        let state = generate_state();
        assert!(state.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_generate_api_key_prefix() {
        let key = generate_api_key_value();
        assert!(key.starts_with("ccui_"));
        assert_eq!(key.len(), "ccui_".len() + 64);
    }

    #[test]
    fn test_generate_and_hash_api_key() {
        let key = generate_api_key_value();
        let hash = api_key::hash_api_key(&key, b"test");
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_hash_deterministic() {
        let key = "ccui_test-key-12345";
        let hash1 = api_key::hash_api_key(key, b"test");
        let hash2 = api_key::hash_api_key(key, b"test");
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_decode_jwt_payload() {
        let payload = serde_json::json!({"sub": "user123", "preferred_username": "testuser"});
        let payload_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::to_string(&payload).unwrap().as_bytes());
        let token = format!("header.{payload_b64}.signature");

        let result = decode_jwt_payload(&token).unwrap();
        assert_eq!(result["sub"], "user123");
        assert_eq!(result["preferred_username"], "testuser");
    }

    #[test]
    fn test_decode_jwt_payload_invalid() {
        assert!(decode_jwt_payload("not-a-jwt").is_err());
    }

    #[test]
    fn test_urlencoding() {
        let result = urlencoding("hello world");
        assert_eq!(result, "hello+world");
    }

    #[tokio::test]
    async fn test_pkce_store_insert_and_remove() {
        let store: Arc<Mutex<HashMap<String, PkceEntry>>> = Arc::new(Mutex::new(HashMap::new()));
        let entry = PkceEntry {
            code_verifier: "test-verifier".into(),
            csrf_state: "test-state".into(),
            created_at: Instant::now(),
        };
        store.lock().await.insert("test-state".into(), entry);

        let removed = store.lock().await.remove("test-state");
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().code_verifier, "test-verifier");
    }

    #[tokio::test]
    async fn test_pkce_store_missing_entry() {
        let store: Arc<Mutex<HashMap<String, PkceEntry>>> = Arc::new(Mutex::new(HashMap::new()));
        let removed = store.lock().await.remove("nonexistent");
        assert!(removed.is_none());
    }

    // DB-backed helpers
    async fn create_test_db() -> (hiqlite::Client, tempfile::TempDir) {
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
        crate::db::run_migrations(&client).await.unwrap();
        (client, tmp)
    }

    #[tokio::test]
    async fn test_find_user_by_oidc_sub() {
        let (client, _tmp) = create_test_db().await;
        let sub = "test-subject-123";
        let user_id = uuid::Uuid::new_v4();
        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        client
            .execute(
                "INSERT INTO users (id, username, oidc_subject, is_admin, is_active, has_completed_onboarding, is_technical, created_at) VALUES ($1, $2, $3, 0, 1, 1, 0, $4)",
                hiqlite::params!(user_id.to_string(), "testuser", sub, now),
            )
            .await
            .unwrap();

        let found = find_user_by_oidc_sub(&client, sub).await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().username, "testuser");
    }

    #[tokio::test]
    async fn test_find_user_by_oidc_sub_not_found() {
        let (client, _tmp) = create_test_db().await;
        let found = find_user_by_oidc_sub(&client, "nonexistent").await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_create_user_on_first_login() {
        let (client, _tmp) = create_test_db().await;
        let sub = "new-user-sub";
        let id = uuid::Uuid::new_v4();
        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        client
            .execute(
                "INSERT INTO users (id, username, oidc_subject, is_active, created_at, has_completed_onboarding, is_technical) VALUES ($1, $2, $3, 1, $4, 1, 0)",
                hiqlite::params!(id.to_string(), "newuser", sub, now),
            )
            .await
            .unwrap();

        let found = find_user_by_oidc_sub(&client, sub).await.unwrap();
        assert!(found.is_some());
        let found = found.unwrap();
        assert_eq!(found.username, "newuser");
        assert!(found.is_active);
    }

    #[tokio::test]
    async fn test_update_last_login() {
        let (client, _tmp) = create_test_db().await;
        let sub = "login-user-sub";
        let user_id = uuid::Uuid::new_v4();
        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        client
            .execute(
                "INSERT INTO users (id, username, oidc_subject, is_active, created_at, has_completed_onboarding, is_technical) VALUES ($1, $2, $3, 1, $4, 1, 0)",
                hiqlite::params!(user_id.to_string(), "loginuser", sub, now),
            )
            .await
            .unwrap();

        let updated = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        client
            .execute(
                "UPDATE users SET last_login = $1 WHERE id = $2",
                hiqlite::params!(updated.clone(), user_id.to_string()),
            )
            .await
            .unwrap();

        let mut rows = client
            .query_raw(
                "SELECT last_login FROM users WHERE id = $1",
                hiqlite::params!(user_id.to_string()),
            )
            .await
            .unwrap();
        let stored: Option<String> = rows[0].get("last_login");
        assert_eq!(stored, Some(updated));
    }

    #[tokio::test]
    async fn test_session_create_and_lookup() {
        let (client, _tmp) = create_test_db().await;
        let user_id = uuid::Uuid::new_v4();
        let session_id = uuid::Uuid::new_v4();
        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let future = (chrono::Utc::now() + chrono::Duration::days(30))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let now_clone = now.clone();
        client
            .execute(
                "INSERT INTO users (id, username, oidc_subject, is_active, created_at, has_completed_onboarding, is_technical) VALUES ($1, $2, $3, 1, $4, 1, 0)",
                hiqlite::params!(user_id.to_string(), "sessionuser", "session-sub", now_clone),
            )
            .await
            .unwrap();
        client
            .execute(
                "INSERT INTO sessions (id, user_id, refresh_token, expires_at, created_at) VALUES ($1, $2, $3, $4, $5)",
                hiqlite::params!(session_id.to_string(), user_id.to_string(), "test-refresh-token", future, now),
            )
            .await
            .unwrap();

        let session = find_session(&client, session_id).await.unwrap();
        assert!(session.is_some());
        assert_eq!(session.unwrap().refresh_token, "test-refresh-token");
    }

    #[tokio::test]
    async fn test_session_delete() {
        let (client, _tmp) = create_test_db().await;
        let user_id = uuid::Uuid::new_v4();
        let session_id = uuid::Uuid::new_v4();
        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let future = (chrono::Utc::now() + chrono::Duration::days(30))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let now_clone = now.clone();
        client
            .execute(
                "INSERT INTO users (id, username, oidc_subject, is_active, created_at, has_completed_onboarding, is_technical) VALUES ($1, $2, $3, 1, $4, 1, 0)",
                hiqlite::params!(user_id.to_string(), "deluser", "del-sub", now_clone),
            )
            .await
            .unwrap();
        client
            .execute(
                "INSERT INTO sessions (id, user_id, refresh_token, expires_at, created_at) VALUES ($1, $2, $3, $4, $5)",
                hiqlite::params!(session_id.to_string(), user_id.to_string(), "test-refresh-token", future, now),
            )
            .await
            .unwrap();

        client
            .execute(
                "DELETE FROM sessions WHERE id = $1",
                hiqlite::params!(session_id.to_string()),
            )
            .await
            .unwrap();

        let session = find_session(&client, session_id).await.unwrap();
        assert!(session.is_none());
    }

    #[tokio::test]
    async fn test_complete_onboarding() {
        let (client, _tmp) = create_test_db().await;
        let user_id = uuid::Uuid::new_v4();
        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        client
            .execute(
                "INSERT INTO users (id, username, oidc_subject, is_active, created_at, has_completed_onboarding, is_technical) VALUES ($1, $2, $3, 1, $4, 0, 0)",
                hiqlite::params!(user_id.to_string(), "onboarduser", "onboard-sub", now),
            )
            .await
            .unwrap();

        let user = complete_onboarding(
            &client,
            user_id,
            "Jane Doe".into(),
            "jane@example.com".into(),
            true,
        )
        .await
        .unwrap();

        assert_eq!(user.git_name, Some("Jane Doe".into()));
        assert_eq!(user.git_email, Some("jane@example.com".into()));
        assert!(user.is_technical);
        assert!(user.has_completed_onboarding);
    }
}
