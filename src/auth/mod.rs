use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::extract::FromRequestParts;
use axum::http::header::AUTHORIZATION;
use axum::http::{HeaderMap, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use tokio::sync::RwLock;
use tower::{Layer, Service};
use uuid::Uuid;

use crate::auth::jwks::{fetch_jwks, verify_token, JwksCache, VerifyError};
use crate::auth::session::{AuthMethod, Session};
use crate::config::OmprintConfig;
use crate::db::schema::User;

pub mod api_key;
pub mod jwks;
mod session;

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("invalid or missing token")]
    InvalidToken,
    #[error("user not found")]
    UserNotFound,
    #[error("jwt expired")]
    Expired,
    #[error("JWKS fetch failed: {0}")]
    JwksFetchError(String),
    #[error("JWT validation failed: {0}")]
    JwtValidationError(String),
    #[error("unknown key id")]
    UnknownKid,
    #[error("network error: {0}")]
    Network(String),
}

#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user_id: Uuid,
    pub username: String,
    pub oidc_subject: Option<String>,
    pub is_admin: bool,
    pub is_technical: bool,
}

#[derive(Debug)]
pub enum AuthRejection {
    Unauthorized,
    Forbidden,
}

impl IntoResponse for AuthRejection {
    fn into_response(self) -> Response {
        match self {
            AuthRejection::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "unauthorized" })),
            )
                .into_response(),
            AuthRejection::Forbidden => {
                (StatusCode::FORBIDDEN, Json(json!({ "error": "forbidden" }))).into_response()
            }
        }
    }
}

impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
{
    type Rejection = AuthRejection;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<AuthUser>()
            .cloned()
            .ok_or(AuthRejection::Unauthorized)
    }
}

#[derive(Clone)]
pub struct AuthLayer {
    pub enabled: bool,
    pub db: hiqlite::Client,
    pub jwks_cache: Arc<RwLock<Option<JwksCache>>>,
    pub issuer_url: Option<String>,
    pub client_id: Option<String>,
}

impl AuthLayer {
    pub fn disabled(db: hiqlite::Client) -> Self {
        Self {
            enabled: false,
            db,
            jwks_cache: Arc::new(RwLock::new(None)),
            issuer_url: None,
            client_id: None,
        }
    }

    pub async fn new(
        cfg: &OmprintConfig,
        db: hiqlite::Client,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        if cfg.auth_enabled() {
            let issuer_url = cfg.oidc_issuer_url.as_ref().unwrap();
            let client_id = cfg.oidc_client_id.clone().unwrap_or_default();
            let cache = fetch_jwks(issuer_url, &client_id).await?;
            let cache = Arc::new(RwLock::new(Some(cache)));

            let bg_cache = cache.clone();
            let bg_issuer = issuer_url.clone();
            let bg_client_id = client_id.clone();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600));
                loop {
                    interval.tick().await;
                    match fetch_jwks(&bg_issuer, &bg_client_id).await {
                        Ok(new_cache) => {
                            *bg_cache.write().await = Some(new_cache);
                        }
                        Err(e) => {
                            tracing::warn!("JWKS refresh failed: {e}");
                        }
                    }
                }
            });

            Ok(Self {
                enabled: true,
                db,
                jwks_cache: cache,
                issuer_url: Some(issuer_url.clone()),
                client_id: Some(client_id),
            })
        } else {
            Ok(Self {
                enabled: false,
                db,
                jwks_cache: Arc::new(RwLock::new(None)),
                issuer_url: None,
                client_id: None,
            })
        }
    }

    async fn resolve_user_by_oidc_subject(
        db: &hiqlite::Client,
        subject: &str,
    ) -> Result<User, AuthError> {
        let mut rows = db
            .query_raw(
                "SELECT * FROM users WHERE oidc_subject = $1",
                hiqlite::params!(subject),
            )
            .await
            .map_err(|e| AuthError::Network(e.to_string()))?;
        let user = rows
            .first_mut()
            .map(|row| User::from(&mut *row))
            .ok_or(AuthError::UserNotFound)?;
        Ok(user)
    }

    async fn authenticate(&self, token: &str) -> Result<(User, AuthMethod), AuthError> {
        if api_key::is_api_key_format(token) {
            let user = api_key::verify_api_key(token, &self.db).await?;
            if !user.is_active {
                return Err(AuthError::InvalidToken);
            }
            Ok((user, AuthMethod::ApiKey))
        } else {
            let cache_guard = self.jwks_cache.read().await;
            let cache = cache_guard
                .as_ref()
                .ok_or(AuthError::JwksFetchError("JWKS not initialized".into()))?;

            match verify_token(token, cache) {
                Ok(claims) => {
                    let user = Self::resolve_user_by_oidc_subject(&self.db, &claims.sub).await?;
                    if !user.is_active {
                        return Err(AuthError::InvalidToken);
                    }
                    Ok((user, AuthMethod::Jwt(claims.sub)))
                }
                Err(VerifyError::UnknownKid) => {
                    drop(cache_guard);
                    let Some(issuer_url) = &self.issuer_url else {
                        return Err(AuthError::JwksFetchError("no issuer URL".into()));
                    };
                    let client_id = self.client_id.clone().unwrap_or_default();
                    match fetch_jwks(issuer_url, &client_id).await {
                        Ok(new_cache) => {
                            *self.jwks_cache.write().await = Some(new_cache);
                        }
                        Err(e) => {
                            return Err(AuthError::JwksFetchError(e.to_string()));
                        }
                    }
                    let cache_guard = self.jwks_cache.read().await;
                    let cache = cache_guard.as_ref().ok_or(AuthError::JwksFetchError(
                        "JWKS still uninitialized after refresh".into(),
                    ))?;
                    let claims = verify_token(token, cache).map_err(map_verify_error)?;
                    let user = Self::resolve_user_by_oidc_subject(&self.db, &claims.sub).await?;
                    if !user.is_active {
                        return Err(AuthError::InvalidToken);
                    }
                    Ok((user, AuthMethod::Jwt(claims.sub)))
                }
                Err(e) => Err(map_verify_error(e)),
            }
        }
    }
}

impl<S> Layer<S> for AuthLayer {
    type Service = AuthMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        AuthMiddleware {
            inner,
            layer: self.clone(),
        }
    }
}

#[derive(Clone)]
pub struct AuthMiddleware<S> {
    inner: S,
    layer: AuthLayer,
}

impl<S> Service<Request<axum::body::Body>> for AuthMiddleware<S>
where
    S: Service<Request<axum::body::Body>, Response = Response> + Send + Clone + 'static,
    S::Future: Send,
{
    type Response = Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: Request<axum::body::Body>) -> Self::Future {
        if !self.layer.enabled {
            return Box::pin(self.inner.call(request));
        }

        let layer = self.layer.clone();
        let mut inner = self.inner.clone();

        Box::pin(async move {
            let Some(token) = extract_bearer_token(request.headers()).map(|s| s.to_string()) else {
                return Ok(AuthRejection::Unauthorized.into_response());
            };

            match layer.authenticate(&token).await {
                Ok((user, method)) => {
                    let session = Session::new(user, method);
                    if !session::validate_session(&session) {
                        return Ok(AuthRejection::Unauthorized.into_response());
                    }
                    let auth_user = AuthUser {
                        user_id: session.user.id,
                        username: session.user.username.clone(),
                        oidc_subject: session.user.oidc_subject.clone(),
                        is_admin: session.user.is_admin,
                        is_technical: session.user.is_technical,
                    };
                    let (mut parts, body) = request.into_parts();
                    parts.extensions.insert(auth_user);
                    let request = Request::from_parts(parts, body);
                    inner.call(request).await
                }
                Err(e) => {
                    tracing::debug!("Authentication failed: {e}");
                    Ok(AuthRejection::Unauthorized.into_response())
                }
            }
        })
    }
}

fn extract_bearer_token(headers: &HeaderMap) -> Option<&str> {
    let auth_header = headers.get(AUTHORIZATION)?;
    let auth_str = auth_header.to_str().ok()?;
    auth_str.strip_prefix("Bearer ")
}

fn map_verify_error(e: VerifyError) -> AuthError {
    match e {
        VerifyError::UnknownKid => AuthError::UnknownKid,
        VerifyError::InvalidToken(s) => AuthError::JwtValidationError(s),
        VerifyError::Expired => AuthError::Expired,
        VerifyError::WrongIssuer(s) => AuthError::JwtValidationError(s),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use axum::routing::get;
    use axum::Router;
    use jsonwebtoken::{encode, EncodingKey, Header};
    use std::collections::HashMap;
    use tower::ServiceExt;

    use crate::auth::jwks::base64url_encode;

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
        crate::db::run_migrations(&client).await.unwrap();
        (client, tmp)
    }

    #[tokio::test]
    async fn test_auth_layer_disabled_passes_through() {
        let (client, _tmp) = make_client().await;
        let auth_layer = AuthLayer::disabled(client);

        let app = Router::new()
            .route("/", get(|| async { "ok" }))
            .layer(auth_layer);

        let req = Request::builder().uri("/").body(Body::empty()).unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);
    }

    #[tokio::test]
    async fn test_auth_layer_missing_header_returns_401() {
        let (client, _tmp) = make_client().await;
        let auth_layer = AuthLayer {
            enabled: true,
            db: client,
            jwks_cache: Arc::new(RwLock::new(None)),
            issuer_url: Some("http://localhost".into()),
            client_id: Some("test-client".into()),
        };

        let app = Router::new()
            .route("/", get(|| async { "ok" }))
            .layer(auth_layer);

        let req = Request::builder().uri("/").body(Body::empty()).unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_auth_layer_jwt_success_attaches_user() {
        let (client, _tmp) = make_client().await;

        let key = b"test-hmac-secret-key-32-bytes-long!";
        let kid = "test-key-1";

        let jwk: jsonwebtoken::jwk::Jwk = serde_json::from_value(serde_json::json!({
            "kty": "oct",
            "k": base64url_encode(key),
            "kid": kid,
            "alg": "HS256"
        }))
        .unwrap();

        let mut keys = HashMap::new();
        keys.insert(kid.to_string(), jwk);

        let jwks_cache = Arc::new(RwLock::new(Some(JwksCache {
            keys,
            issuer: "test-issuer".to_string(),
            client_id: "test-client".to_string(),
        })));

        let auth_layer = AuthLayer {
            enabled: true,
            db: client.clone(),
            jwks_cache,
            issuer_url: Some("test-issuer".to_string()),
            client_id: Some("test-client".to_string()),
        };

        let user_id = Uuid::new_v4();
        client
            .execute(
                "INSERT INTO users (id, username, oidc_subject, is_admin, is_technical, has_completed_onboarding, token_version) VALUES ($1, $2, $3, $4, $5, $6, $7)",
                hiqlite::params!(
                    user_id.to_string(),
                    "jwt-user",
                    "user-123",
                    0i64,
                    0i64,
                    1i64,
                    0i64
                ),
            )
            .await
            .unwrap();

        let claims = crate::auth::jwks::Claims {
            sub: "user-123".to_string(),
            iss: "test-issuer".to_string(),
            aud: serde_json::json!("test-client"),
            exp: 9_999_999_999,
            iat: Some(1_000_000_000),
        };
        let mut header = Header::new(jsonwebtoken::Algorithm::HS256);
        header.kid = Some(kid.to_string());
        let token = encode(&header, &claims, &EncodingKey::from_secret(key)).unwrap();

        async fn auth_handler(auth: AuthUser) -> impl axum::response::IntoResponse {
            auth.username
        }

        let app = Router::new()
            .route("/", get(auth_handler))
            .layer(auth_layer);

        let req = Request::builder()
            .uri("/")
            .header("Authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_auth_layer_jwt_user_not_found_returns_401() {
        let (client, _tmp) = make_client().await;

        let key = b"test-hmac-secret-key-32-bytes-long!";
        let kid = "test-key-1";

        let jwk: jsonwebtoken::jwk::Jwk = serde_json::from_value(serde_json::json!({
            "kty": "oct",
            "k": base64url_encode(key),
            "kid": kid,
            "alg": "HS256"
        }))
        .unwrap();

        let mut keys = HashMap::new();
        keys.insert(kid.to_string(), jwk);

        let jwks_cache = Arc::new(RwLock::new(Some(JwksCache {
            keys,
            issuer: "test-issuer".to_string(),
            client_id: "test-client".to_string(),
        })));

        let auth_layer = AuthLayer {
            enabled: true,
            db: client,
            jwks_cache,
            issuer_url: Some("test-issuer".to_string()),
            client_id: Some("test-client".to_string()),
        };

        let claims = crate::auth::jwks::Claims {
            sub: "nonexistent-user".to_string(),
            iss: "test-issuer".to_string(),
            aud: serde_json::json!("test-client"),
            exp: 9_999_999_999,
            iat: Some(1_000_000_000),
        };
        let mut header = Header::new(jsonwebtoken::Algorithm::HS256);
        header.kid = Some(kid.to_string());
        let token = encode(&header, &claims, &EncodingKey::from_secret(key)).unwrap();

        let app = Router::new()
            .route("/", get(|| async { "ok" }))
            .layer(auth_layer);

        let req = Request::builder()
            .uri("/")
            .header("Authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_auth_layer_api_key_success_attaches_user() {
        let (client, _tmp) = make_client().await;

        let api_key = "ccui_test-api-key-for-middleware";
        let hash = crate::auth::api_key::hash_api_key(api_key);

        let user_id = Uuid::new_v4();
        client
            .execute(
                "INSERT INTO users (id, username, api_key_hash, is_admin, is_technical, has_completed_onboarding, token_version) VALUES ($1, $2, $3, $4, $5, $6, $7)",
                hiqlite::params!(
                    user_id.to_string(),
                    "api-key-user",
                    hash,
                    0i64,
                    0i64,
                    1i64,
                    0i64
                ),
            )
            .await
            .unwrap();

        // Auth enabled, but jwks_cache can be None since API key path doesn't touch it
        let auth_layer = AuthLayer {
            enabled: true,
            db: client,
            jwks_cache: Arc::new(RwLock::new(None)),
            issuer_url: None,
            client_id: None,
        };

        async fn auth_handler(auth: AuthUser) -> impl axum::response::IntoResponse {
            auth.username
        }

        let app = Router::new()
            .route("/", get(auth_handler))
            .layer(auth_layer);

        let req = Request::builder()
            .uri("/")
            .header("Authorization", format!("Bearer {api_key}"))
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_auth_user_extractor_present() {
        let user = AuthUser {
            user_id: Uuid::new_v4(),
            username: "test-user".into(),
            oidc_subject: None,
            is_admin: false,
            is_technical: false,
        };
        let mut parts = Request::new(Body::empty()).into_parts().0;
        parts.extensions.insert(user.clone());
        let result = AuthUser::from_request_parts(&mut parts, &()).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().username, "test-user");
    }

    #[tokio::test]
    async fn test_auth_user_extractor_missing() {
        let mut parts = Request::new(Body::empty()).into_parts().0;
        let result = AuthUser::from_request_parts(&mut parts, &()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_auth_rejection_unauthorized_response() {
        let resp = AuthRejection::Unauthorized.into_response();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_auth_rejection_forbidden_response() {
        let resp = AuthRejection::Forbidden.into_response();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_extract_bearer_token() {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, "Bearer my-token".parse().unwrap());
        assert_eq!(extract_bearer_token(&headers), Some("my-token"));

        let empty = HeaderMap::new();
        assert_eq!(extract_bearer_token(&empty), None);

        let mut bad = HeaderMap::new();
        bad.insert(AUTHORIZATION, "Basic dGVzdDpwYXNz".parse().unwrap());
        assert_eq!(extract_bearer_token(&bad), None);
    }
}
