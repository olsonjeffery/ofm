use axum::body::Body;
use axum::extract::State;
use axum::http::header::{AUTHORIZATION, COOKIE};
use axum::http::{Request, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Router;

use http_body_util::BodyExt;

use crate::server::state::AppState;

/// Creates an axum Router that proxies all requests (via fallback) to the
/// configured rauthy target, preserving the `/auth` prefix in the forwarded URL.
pub fn rauthy_proxy_router() -> Router<AppState> {
    Router::new().fallback(proxy_handler)
}

async fn proxy_handler(State(state): State<AppState>, req: Request<Body>) -> Response {
    let base_url = match &state.rauthy_base_url {
        Some(url) => url.clone(),
        None => {
            return (StatusCode::SERVICE_UNAVAILABLE, "rauthy not configured").into_response();
        }
    };

    let client = reqwest::Client::new();
    let path = req.uri().path();
    let query = req
        .uri()
        .query()
        .map(|q| format!("?{q}"))
        .unwrap_or_default();
    let target = format!("{}/auth{}{}", base_url, path, query);

    let method = req.method().clone();
    let mut headers = req.headers().clone();
    headers.remove(AUTHORIZATION);
    headers.remove(COOKIE);

    let body_bytes = match req.into_body().collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(e) => {
            return (StatusCode::BAD_REQUEST, format!("failed to read body: {e}")).into_response();
        }
    };

    let resp = match client
        .request(method, &target)
        .headers(headers)
        .body(body_bytes)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return (StatusCode::BAD_GATEWAY, format!("proxy error: {e}")).into_response();
        }
    };

    let status = resp.status();
    let resp_headers = resp.headers().clone();
    let resp_body = match resp.bytes().await {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                format!("failed to read proxy response: {e}"),
            )
                .into_response();
        }
    };

    let mut response = Response::builder()
        .status(status)
        .body(Body::from(resp_body))
        .unwrap();
    *response.headers_mut() = resp_headers;
    response
}
