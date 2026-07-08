use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use cookie::Key;
use tower::{Layer, Service};
use tracing::{debug, warn};
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::db::schema::SessionDb;

fn lookup_session(
    db: &hiqlite::Client,
    session_id: Uuid,
) -> impl Future<Output = Option<SessionDb>> {
    let db = db.clone();
    async move {
        let mut rows = db
            .query_raw(
                "SELECT * FROM sessions WHERE id = $1",
                hiqlite::params!(session_id.to_string()),
            )
            .await
            .ok()?;
        let session = rows.first_mut()?;
        Some(SessionDb::from(&mut *session))
    }
}

fn lookup_user_active(
    db: &hiqlite::Client,
    user_id: Uuid,
) -> impl Future<Output = Option<AuthUser>> {
    let db = db.clone();
    async move {
        let mut rows = db
            .query_raw(
                "SELECT * FROM users WHERE id = $1",
                hiqlite::params!(user_id.to_string()),
            )
            .await
            .ok()?;
        let user_raw = rows.first_mut()?;
        let user = crate::db::schema::User::from(&mut *user_raw);
        if !user.is_active {
            return None;
        }
        Some(AuthUser::from(user))
    }
}

#[derive(Clone)]
pub struct WebappAuthLayer {
    enabled: bool,
    db: hiqlite::Client,
    cookie_key: Key,
}

impl WebappAuthLayer {
    pub fn new(db: hiqlite::Client, cookie_key: Key) -> Self {
        Self {
            enabled: true,
            db,
            cookie_key,
        }
    }

    pub fn disabled(db: hiqlite::Client, cookie_key: Key) -> Self {
        Self {
            enabled: false,
            db,
            cookie_key,
        }
    }
}

impl<S> Layer<S> for WebappAuthLayer {
    type Service = WebappAuthMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        WebappAuthMiddleware {
            inner,
            layer: self.clone(),
        }
    }
}

#[derive(Clone)]
pub struct WebappAuthMiddleware<S> {
    inner: S,
    layer: WebappAuthLayer,
}

impl<S> Service<Request<Body>> for WebappAuthMiddleware<S>
where
    S: Service<Request<Body>, Response = Response> + Send + Clone + 'static,
    S::Future: Send,
{
    type Response = Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: Request<Body>) -> Self::Future {
        if !self.layer.enabled {
            let auth_user = AuthUser {
                user_id: uuid::Uuid::nil(),
                username: String::new(),
                oidc_subject: None,
                is_admin: false,
                is_technical: false,
            };
            let (mut parts, body) = request.into_parts();
            parts.extensions.insert(auth_user);
            let request = Request::from_parts(parts, body);
            return Box::pin(self.inner.call(request));
        }

        let db = self.layer.db.clone();
        let cookie_key = self.layer.cookie_key.clone();
        let mut inner = self.inner.clone();

        Box::pin(async move {
            let session_id = match extract_session_from_cookies(request.headers(), &cookie_key) {
                Some(id) => id,
                None => {
                    warn!("WebappAuth: no valid session cookie found in request");
                    return Ok(redirect_to_login());
                }
            };

            let session = match lookup_session(&db, session_id).await {
                Some(s) => s,
                None => {
                    warn!("WebappAuth: session {session_id} not found in DB");
                    return Ok(redirect_to_login());
                }
            };

            let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
            if session.expires_at < now {
                warn!(
                    "WebappAuth: session {session_id} expired (expires_at={}, now={})",
                    session.expires_at, now
                );
                return Ok(redirect_to_login());
            }

            let auth_user = match lookup_user_active(&db, session.user_id).await {
                Some(u) => u,
                None => {
                    warn!(
                        "WebappAuth: user {} for session {session_id} not found or inactive",
                        session.user_id
                    );
                    return Ok(redirect_to_login());
                }
            };

            let (mut parts, body) = request.into_parts();
            parts.extensions.insert(auth_user);
            let request = Request::from_parts(parts, body);
            inner.call(request).await
        })
    }
}

fn redirect_to_login() -> Response {
    (StatusCode::FOUND, [("Location", "/webapp/login")]).into_response()
}

fn extract_session_from_cookies(headers: &axum::http::HeaderMap, key: &Key) -> Option<Uuid> {
    let mut jar = cookie::CookieJar::new();
    for header in headers.get_all(header::COOKIE) {
        let s = header.to_str().ok()?;
        for part in s.split(';') {
            if let Ok(c) = cookie::Cookie::parse_encoded(part.to_owned()) {
                jar.add_original(c);
            }
        }
    }

    let private = jar.private(key);
    let session_cookie = match private.get("omprint_session") {
        Some(c) => c,
        None => {
            debug!("extract_session_from_cookies: failed to decrypt omprint_session cookie");
            return None;
        }
    };
    Uuid::parse_str(session_cookie.value()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use axum::routing::get;
    use axum::Router;
    use tower::ServiceExt;

    async fn create_test_app() -> (Router, Key, hiqlite::Client) {
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
        let key = Key::generate();

        let layer = WebappAuthLayer::new(client.clone(), key.clone());
        let app = Router::new()
            .route("/protected", get(|| async { "ok" }))
            .layer(layer);
        (app, key, client)
    }

    fn make_encrypted_cookie(key: &Key, name: &str, value: &str) -> String {
        let mut jar = cookie::CookieJar::new();
        {
            let mut private = jar.private_mut(key);
            private.add(cookie::Cookie::new(name.to_owned(), value.to_owned()));
        }
        let c = jar.get(name).unwrap();
        format!("{}={}", c.name(), c.value())
    }

    #[tokio::test]
    async fn test_missing_cookie_redirects_to_login() {
        let (app, _key, _client) = create_test_app().await;
        let req = Request::builder()
            .uri("/protected")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FOUND);
        assert_eq!(
            resp.headers().get("location").unwrap().to_str().unwrap(),
            "/webapp/login"
        );
    }

    #[tokio::test]
    async fn test_valid_session_passes_through() {
        let (app, key, client) = create_test_app().await;
        let user_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();
        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let future = (chrono::Utc::now() + chrono::Duration::days(30))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();

        client
            .execute(
                "INSERT INTO users (id, username, oidc_subject, is_active, created_at, has_completed_onboarding, is_technical) VALUES ($1, $2, $3, 1, $4, 1, 0)",
                hiqlite::params!(user_id.to_string(), "testuser", "test-sub", now.clone()),
            )
            .await
            .unwrap();

        client
            .execute(
                "INSERT INTO sessions (id, user_id, refresh_token, expires_at, created_at) VALUES ($1, $2, $3, $4, $5)",
                hiqlite::params!(session_id.to_string(), user_id.to_string(), "refresh-token", future, now),
            )
            .await
            .unwrap();

        let cookie_str = make_encrypted_cookie(&key, "omprint_session", &session_id.to_string());
        let req = Request::builder()
            .uri("/protected")
            .header("Cookie", cookie_str)
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_expired_session_redirects_to_login() {
        let (app, key, client) = create_test_app().await;
        let user_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();
        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let past = (chrono::Utc::now() - chrono::Duration::days(1))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();

        client
            .execute(
                "INSERT INTO users (id, username, oidc_subject, is_active, created_at, has_completed_onboarding, is_technical) VALUES ($1, $2, $3, 1, $4, 1, 0)",
                hiqlite::params!(user_id.to_string(), "testuser", "test-sub", now.clone()),
            )
            .await
            .unwrap();

        client
            .execute(
                "INSERT INTO sessions (id, user_id, refresh_token, expires_at, created_at) VALUES ($1, $2, $3, $4, $5)",
                hiqlite::params!(session_id.to_string(), user_id.to_string(), "refresh-token", past, now),
            )
            .await
            .unwrap();

        let cookie_str = make_encrypted_cookie(&key, "omprint_session", &session_id.to_string());
        let req = Request::builder()
            .uri("/protected")
            .header("Cookie", cookie_str)
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FOUND);
        assert_eq!(
            resp.headers().get("location").unwrap().to_str().unwrap(),
            "/webapp/login"
        );
    }
}
