use sha2::{Digest, Sha256};

use crate::auth::AuthError;
use crate::db::schema::User;

pub const API_KEY_PREFIX: &str = "ccui_";

pub fn is_api_key_format(token: &str) -> bool {
    token.starts_with(API_KEY_PREFIX)
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

pub fn hash_api_key(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    bytes_to_hex(&hasher.finalize())
}

pub async fn verify_api_key(token: &str, db: &hiqlite::Client) -> Result<User, AuthError> {
    let hash = hash_api_key(token);

    let user = {
        let mut rows = db
            .query_raw(
                "SELECT * FROM users WHERE api_key_hash = $1",
                hiqlite::params!(hash),
            )
            .await
            .map_err(|e| AuthError::Network(e.to_string()))?;

        let user = rows
            .first_mut()
            .map(|row| User::from(&mut *row))
            .ok_or(AuthError::InvalidToken)?;
        user
    };

    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let _ = db
        .execute(
            "UPDATE users SET api_key_last_used_at = $1 WHERE id = $2",
            hiqlite::params!(now, user.id.to_string()),
        )
        .await;

    Ok(user)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

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

    #[tokio::test]
    async fn test_verify_api_key_lookup_success() {
        let (client, _tmp) = make_client().await;
        let key = "ccui_test-api-key-12345";
        let hash = hash_api_key(key);

        let user_id = uuid::Uuid::new_v4();
        client
            .execute(
                "INSERT INTO users (id, username, api_key_hash, is_admin, is_technical, has_completed_onboarding, token_version) VALUES ($1, $2, $3, $4, $5, $6, $7)",
                hiqlite::params!(
                    user_id.to_string(),
                    "test-user-key",
                    hash.clone(),
                    0i64,
                    0i64,
                    1i64,
                    0i64
                ),
            )
            .await
            .unwrap();

        let user = verify_api_key(key, &client).await.unwrap();
        assert_eq!(user.id, user_id);
        assert_eq!(user.username, "test-user-key");
        assert_eq!(user.api_key_hash, Some(hash.clone()));
    }

    #[tokio::test]
    async fn test_verify_api_key_lookup_failure() {
        let (client, _tmp) = make_client().await;
        let result = verify_api_key("ccui_nonexistent-key", &client).await;
        assert!(matches!(result, Err(AuthError::InvalidToken)));
    }

    #[tokio::test]
    async fn test_api_key_last_used_update() {
        let (client, _tmp) = make_client().await;
        let key = "ccui_test-key-last-used";
        let hash = hash_api_key(key);

        let user_id = uuid::Uuid::new_v4();
        client
            .execute(
                "INSERT INTO users (id, username, api_key_hash, is_admin, is_technical, has_completed_onboarding, token_version) VALUES ($1, $2, $3, $4, $5, $6, $7)",
                hiqlite::params!(
                    user_id.to_string(),
                    "test-user-last-used",
                    hash,
                    0i64,
                    0i64,
                    1i64,
                    0i64
                ),
            )
            .await
            .unwrap();

        let _user = verify_api_key(key, &client).await.unwrap();

        let mut rows = client
            .query_raw(
                "SELECT api_key_last_used_at FROM users WHERE id = $1",
                hiqlite::params!(user_id.to_string()),
            )
            .await
            .unwrap();
        let last_used: Option<String> = rows[0].get("api_key_last_used_at");
        assert!(last_used.is_some(), "api_key_last_used_at should be set");
        assert!(!last_used.as_ref().unwrap().is_empty());
    }

    #[test]
    fn test_is_api_key_format() {
        assert!(is_api_key_format("ccui_abcdef123456"));
        assert!(!is_api_key_format("eyJhbGciOiJSUzI1NiJ9"));
        assert!(!is_api_key_format(""));
        assert!(!is_api_key_format("api_abcdef"));
    }

    #[test]
    fn test_hash_api_key_deterministic() {
        let key = "ccui_test-key-12345";
        let hash1 = hash_api_key(key);
        let hash2 = hash_api_key(key);
        assert_eq!(hash1, hash2);
        assert_eq!(hash1.len(), 64);
    }

    #[test]
    fn test_hash_api_key_different_keys() {
        let hash1 = hash_api_key("ccui_key_one");
        let hash2 = hash_api_key("ccui_key_two");
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_bytes_to_hex() {
        assert_eq!(bytes_to_hex(b""), "");
        assert_eq!(bytes_to_hex(b"\x00"), "00");
        assert_eq!(bytes_to_hex(b"\xff"), "ff");
        assert_eq!(bytes_to_hex(b"hello"), "68656c6c6f");
    }
}
