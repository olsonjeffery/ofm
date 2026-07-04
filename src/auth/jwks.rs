use std::collections::HashMap;

use jsonwebtoken::jwk::{Jwk, KeyAlgorithm};
use jsonwebtoken::{Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};

use crate::auth::AuthError;

#[derive(Debug, Clone)]
pub struct JwksCache {
    pub keys: HashMap<String, Jwk>,
    pub issuer: String,
    pub client_id: String,
}

#[derive(Debug, Deserialize)]
struct OidcDiscovery {
    issuer: String,
    jwks_uri: String,
}

#[derive(Debug, Deserialize)]
struct JwksResponse {
    keys: Vec<Jwk>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub iss: String,
    pub aud: serde_json::Value,
    pub exp: usize,
    pub iat: Option<usize>,
}

#[derive(Debug)]
pub enum VerifyError {
    UnknownKid,
    InvalidToken(String),
    Expired,
    WrongIssuer(String),
}

pub async fn fetch_jwks(issuer_url: &str, client_id: &str) -> Result<JwksCache, AuthError> {
    let client = reqwest::Client::new();

    let discovery_url = format!(
        "{}/.well-known/openid-configuration",
        issuer_url.trim_end_matches('/')
    );
    let disc_resp = client
        .get(&discovery_url)
        .send()
        .await
        .map_err(|e| AuthError::JwksFetchError(e.to_string()))?;
    let disc: OidcDiscovery = disc_resp
        .json()
        .await
        .map_err(|e| AuthError::JwksFetchError(e.to_string()))?;

    if disc.issuer != issuer_url {
        return Err(AuthError::JwksFetchError(format!(
            "issuer mismatch: expected {issuer_url}, got {}",
            disc.issuer
        )));
    }

    let jwks_resp = client
        .get(&disc.jwks_uri)
        .send()
        .await
        .map_err(|e| AuthError::JwksFetchError(e.to_string()))?;
    let jwks: JwksResponse = jwks_resp
        .json()
        .await
        .map_err(|e| AuthError::JwksFetchError(e.to_string()))?;

    let keys: HashMap<String, Jwk> = jwks
        .keys
        .into_iter()
        .filter_map(|jwk| jwk.common.key_id.clone().map(|kid| (kid, jwk)))
        .collect();

    Ok(JwksCache {
        keys,
        issuer: issuer_url.to_string(),
        client_id: client_id.to_string(),
    })
}

pub fn verify_token(token: &str, cache: &JwksCache) -> Result<Claims, VerifyError> {
    let header =
        jsonwebtoken::decode_header(token).map_err(|e| VerifyError::InvalidToken(e.to_string()))?;

    let kid = header
        .kid
        .ok_or_else(|| VerifyError::InvalidToken("missing kid in header".into()))?;

    let jwk = cache.keys.get(&kid).ok_or(VerifyError::UnknownKid)?;

    let decoding_key =
        DecodingKey::from_jwk(jwk).map_err(|e| VerifyError::InvalidToken(e.to_string()))?;

    let alg = match jwk.common.key_algorithm {
        Some(KeyAlgorithm::HS256) => Algorithm::HS256,
        Some(KeyAlgorithm::HS384) => Algorithm::HS384,
        Some(KeyAlgorithm::HS512) => Algorithm::HS512,
        Some(KeyAlgorithm::RS256) => Algorithm::RS256,
        Some(KeyAlgorithm::RS384) => Algorithm::RS384,
        Some(KeyAlgorithm::RS512) => Algorithm::RS512,
        Some(KeyAlgorithm::ES256) => Algorithm::ES256,
        Some(KeyAlgorithm::ES384) => Algorithm::ES384,
        Some(KeyAlgorithm::PS256) => Algorithm::PS256,
        Some(KeyAlgorithm::PS384) => Algorithm::PS384,
        Some(KeyAlgorithm::PS512) => Algorithm::PS512,
        Some(KeyAlgorithm::EdDSA) => Algorithm::EdDSA,
        _ => Algorithm::RS256,
    };
    let mut validation = Validation::new(alg);
    validation.set_issuer(&[&cache.issuer]);
    validation.set_audience(&[&cache.client_id]);

    let token_data =
        jsonwebtoken::decode::<Claims>(token, &decoding_key, &validation).map_err(|e| {
            match e.kind() {
                jsonwebtoken::errors::ErrorKind::ExpiredSignature => VerifyError::Expired,
                jsonwebtoken::errors::ErrorKind::InvalidIssuer => {
                    VerifyError::WrongIssuer("invalid issuer".into())
                }
                _ => VerifyError::InvalidToken(e.to_string()),
            }
        })?;

    Ok(token_data.claims)
}

pub(crate) fn base64url_encode(input: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut result = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        }
        if chunk.len() > 2 {
            result.push(CHARS[(triple & 0x3F) as usize] as char);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{encode, EncodingKey, Header};

    fn make_test_jwk(key: &[u8], kid: &str) -> Jwk {
        serde_json::from_value(serde_json::json!({
            "kty": "oct",
            "k": base64url_encode(key),
            "kid": kid,
            "alg": "HS256"
        }))
        .unwrap()
    }

    fn make_test_cache(key: &[u8], kid: &str, issuer: &str, client_id: &str) -> JwksCache {
        let jwk = make_test_jwk(key, kid);
        let mut keys = HashMap::new();
        keys.insert(kid.to_string(), jwk);
        JwksCache {
            keys,
            issuer: issuer.to_string(),
            client_id: client_id.to_string(),
        }
    }

    #[test]
    fn test_verify_token_valid() {
        let key = b"test-hmac-secret-key-32-bytes-long!";
        let kid = "test-key-1";
        let cache = make_test_cache(key, kid, "test-issuer", "test-client");

        let claims = Claims {
            sub: "user-123".to_string(),
            iss: "test-issuer".to_string(),
            aud: serde_json::json!("test-client"),
            exp: 9_999_999_999,
            iat: Some(1_000_000_000),
        };

        let mut header = Header::new(Algorithm::HS256);
        header.kid = Some(kid.to_string());
        let token = encode(&header, &claims, &EncodingKey::from_secret(key)).unwrap();

        let result = verify_token(&token, &cache);
        if let Err(ref e) = result {
            panic!("verify_token failed: {:?}", e);
        }
        assert_eq!(result.unwrap().sub, "user-123");
    }

    #[test]
    fn test_verify_token_expired() {
        let key = b"test-hmac-secret-key-32-bytes-long!";
        let kid = "test-key-1";
        let cache = make_test_cache(key, kid, "test-issuer", "test-client");

        let claims = Claims {
            sub: "user-123".to_string(),
            iss: "test-issuer".to_string(),
            aud: serde_json::json!("test-client"),
            exp: 1_000_000_000,
            iat: Some(1_000_000_000),
        };

        let mut header = Header::new(Algorithm::HS256);
        header.kid = Some(kid.to_string());
        let token = encode(&header, &claims, &EncodingKey::from_secret(key)).unwrap();

        let result = verify_token(&token, &cache);
        assert!(matches!(result, Err(VerifyError::Expired)));
    }

    #[test]
    fn test_verify_token_wrong_issuer() {
        let key = b"test-hmac-secret-key-32-bytes-long!";
        let kid = "test-key-1";
        let cache = make_test_cache(key, kid, "correct-issuer", "test-client");

        let claims = Claims {
            sub: "user-123".to_string(),
            iss: "wrong-issuer".to_string(),
            aud: serde_json::json!("test-client"),
            exp: 9_999_999_999,
            iat: Some(1_000_000_000),
        };

        let mut header = Header::new(Algorithm::HS256);
        header.kid = Some(kid.to_string());
        let token = encode(&header, &claims, &EncodingKey::from_secret(key)).unwrap();

        let result = verify_token(&token, &cache);
        assert!(matches!(result, Err(VerifyError::WrongIssuer(_))));
    }

    #[test]
    fn test_verify_token_unknown_kid() {
        let key = b"test-hmac-secret-key-32-bytes-long!";
        let cache = make_test_cache(key, "key-in-cache", "test-issuer", "test-client");

        let claims = Claims {
            sub: "user-123".to_string(),
            iss: "test-issuer".to_string(),
            aud: serde_json::json!("test-client"),
            exp: 9_999_999_999,
            iat: Some(1_000_000_000),
        };

        let mut header = Header::new(Algorithm::HS256);
        header.kid = Some("unknown-kid".to_string());
        let token = encode(&header, &claims, &EncodingKey::from_secret(key)).unwrap();

        let result = verify_token(&token, &cache);
        assert!(matches!(result, Err(VerifyError::UnknownKid)));
    }

    #[tokio::test]
    async fn test_fetch_jwks_parses_discovery() {
        use axum::{routing::get, Json, Router};
        use serde_json::json;

        let app = Router::new()
            .route(
                "/.well-known/openid-configuration",
                get(|| async {
                    Json(json!({
                        "issuer": "http://localhost:PORT",
                        "jwks_uri": "http://localhost:PORT/jwks"
                    }))
                }),
            )
            .route("/jwks", get(|| async { Json(json!({ "keys": [] })) }));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let result = fetch_jwks(&format!("http://{}", addr), "client-id").await;
        assert!(result.is_err(), "should fail due to issuer mismatch");
    }

    #[tokio::test]
    async fn test_fetch_jwks_issuer_mismatch() {
        use axum::{routing::get, Json, Router};
        use serde_json::json;

        let app = Router::new()
            .route(
                "/.well-known/openid-configuration",
                get(|| async {
                    Json(json!({
                        "issuer": "http://different-issuer",
                        "jwks_uri": "http://localhost:PORT/jwks"
                    }))
                }),
            )
            .route("/jwks", get(|| async { Json(json!({ "keys": [] })) }));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let result = fetch_jwks(&format!("http://{}", addr), "client-id").await;
        assert!(matches!(result, Err(AuthError::JwksFetchError(_))));
    }
}
