//! Reverse proxy for the embedded rauthy instance.
//!
//! The browser reaches rauthy exclusively through OFM's `/auth/*` route, so
//! OFM can bind a single port while still hosting rauthy on a private port.
//! All traffic is forwarded verbatim to `http://127.0.0.1:{rauthy_port}` —
//! method, headers, path, query, and body — which satisfies "bind to one port,
//! accept hosts on different ports".
//!
//! Header handling:
//! - Hop-by-hop headers (`connection`, `keep-alive`, `proxy-*`, `te`,
//!   `trailer`, `transfer-encoding`, `upgrade`) are stripped on both the
//!   outbound request and the inbound response.
//! - The original incoming `Host` header is preserved (rauthy uses it for
//!   cookie domain selection).
//! - `X-Forwarded-Host` / `X-Forwarded-Proto` are passed through when present.
//! - The peer IP is appended to `X-Forwarded-For` so rauthy (in `proxy_mode`)
//!   sees the real client IP when OFM itself sits behind an external proxy.

use axum::body::Body;
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderValue, Request, Response, StatusCode, Uri};
use axum::Router;
use http_body_util::BodyExt;
use std::net::SocketAddr;

type ProxyClient =
    hyper_util::client::legacy::Client<hyper_util::client::legacy::connect::HttpConnector, Body>;

const HOP_BY_HOP: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;

/// Build a standalone `/auth` router that proxies to the rauthy container on
/// `rauthy_port`. Returns a `Router<()>` so it can be nested into any state.
pub fn rauthy_proxy_router(rauthy_port: u16) -> Router {
    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build(hyper_util::client::legacy::connect::HttpConnector::new());
    Router::new()
        .fallback(proxy_handler)
        .with_state((client, rauthy_port))
}

async fn proxy_handler(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    State((client, rauthy_port)): State<(ProxyClient, u16)>,
    req: Request<Body>,
) -> Response<Body> {
    let (parts, body) = req.into_parts();

    let body_bytes = match axum::body::to_bytes(body, MAX_BODY_BYTES).await {
        Ok(bytes) => bytes,
        Err(e) => {
            tracing::warn!(error = %e, "rauthy proxy: failed to buffer request body");
            return Response::builder()
                .status(StatusCode::BAD_GATEWAY)
                .body(Body::from("failed to buffer request body"))
                .unwrap();
        }
    };

    let path_and_query = parts
        .uri
        .path_and_query()
        .map(|pq| pq.as_str().to_string())
        .unwrap_or_default();
    // axum's `nest_service("/auth", ...)` strips the `/auth` prefix before
    // calling this handler, so re-prepend it for the upstream request —
    // rauthy serves everything under `/auth/v1/*`.
    let target_uri =
        match format!("http://127.0.0.1:{rauthy_port}/auth{path_and_query}").parse::<Uri>() {
            Ok(uri) => uri,
            Err(e) => {
                tracing::warn!(error = %e, "rauthy proxy: invalid target URI");
                return Response::builder()
                    .status(StatusCode::BAD_GATEWAY)
                    .body(Body::from("invalid target URI"))
                    .unwrap();
            }
        };

    let mut builder = Request::builder().method(parts.method).uri(target_uri);

    if let Some(headers) = builder.headers_mut() {
        for (name, value) in parts.headers.iter() {
            if HOP_BY_HOP.contains(&name.as_str()) {
                continue;
            }
            headers.append(name, value.clone());
        }
        // Append the peer IP to X-Forwarded-For (or start it if absent).
        let xff = parts
            .headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .filter(|s| !s.is_empty());
        let xff_value = match xff {
            Some(existing) => format!("{existing}, {}", peer.ip()),
            None => peer.ip().to_string(),
        };
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_str(&xff_value).unwrap_or_else(|_| HeaderValue::from_static("")),
        );
    }

    let request = match builder.body(Body::from(body_bytes)) {
        Ok(req) => req,
        Err(e) => {
            tracing::warn!(error = %e, "rauthy proxy: failed to build request");
            return Response::builder()
                .status(StatusCode::BAD_GATEWAY)
                .body(Body::from("failed to build request"))
                .unwrap();
        }
    };

    match client.request(request).await {
        Ok(resp) => {
            let (parts, resp_body) = resp.into_parts();
            let mut headers = parts.headers.clone();
            for name in HOP_BY_HOP {
                headers.remove(*name);
            }
            match resp_body
                .collect()
                .await
                .map(|collected| collected.to_bytes())
            {
                Ok(bytes) => Response::builder()
                    .status(parts.status)
                    .version(parts.version)
                    .body(Body::from(bytes))
                    .map(|mut resp| {
                        *resp.headers_mut() = headers;
                        resp
                    })
                    .unwrap_or_else(|_| {
                        Response::builder()
                            .status(StatusCode::BAD_GATEWAY)
                            .body(Body::from("failed to build response"))
                            .unwrap()
                    }),
                Err(e) => {
                    tracing::warn!(error = %e, "rauthy proxy: failed to read response body");
                    Response::builder()
                        .status(StatusCode::BAD_GATEWAY)
                        .body(Body::from("failed to read response body"))
                        .unwrap()
                }
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, "rauthy proxy: upstream request failed");
            Response::builder()
                .status(StatusCode::BAD_GATEWAY)
                .body(Body::from("upstream request failed"))
                .unwrap()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use serde_json::json;

    /// Local echo server that reports the received request (method, path,
    /// query, body, selected headers) as JSON. The response status is taken
    /// from the `status` query parameter (default 200) so both 2xx and
    /// non-2xx relay behaviour can be exercised against one server.
    async fn spawn_echo_server() -> (u16, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = axum::Router::new().fallback(echo_handler);
        let handle = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (addr.port(), handle)
    }

    async fn echo_handler(req: axum::http::Request<Body>) -> Response<Body> {
        let (parts, body) = req.into_parts();
        let bytes = to_bytes(body, 1024 * 1024).await.unwrap();
        let headers: serde_json::Map<String, serde_json::Value> = parts
            .headers
            .iter()
            .map(|(k, v)| (k.to_string(), serde_json::json!(v.to_str().unwrap_or(""))))
            .collect();
        let status = parts
            .uri
            .query()
            .and_then(|q| q.split('&').find_map(|kv| kv.strip_prefix("status=")))
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(200);
        let body = json!({
            "method": parts.method.as_str(),
            "path": parts.uri.path(),
            "query": parts.uri.query(),
            "body": String::from_utf8_lossy(&bytes),
            "headers": headers,
        });
        Response::builder()
            .status(axum::http::StatusCode::from_u16(status).unwrap())
            .header("x-echo-status", status.to_string())
            .header("connection", "keep-alive")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    /// Spawn the proxy router on a random port and return its base URL.
    async fn spawn_proxy(rauthy_port: u16) -> String {
        let app = rauthy_proxy_router(rauthy_port);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap();
        });
        format!("http://{}", addr)
    }

    #[tokio::test]
    async fn test_proxy_forwards_request_and_relays_response() {
        let (echo_port, _echo_handle) = spawn_echo_server().await;
        let proxy_base = spawn_proxy(echo_port).await;

        let resp = reqwest::Client::new()
            .post(format!("{proxy_base}/v1/authorize?code=abc&state=xyz"))
            .header("host", "ofm.example.com")
            .header("x-forwarded-proto", "https")
            .header("x-forwarded-for", "203.0.113.9")
            .body("hello rauthy")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), 200);
        let headers_before = resp.headers().get("connection").cloned();
        let body_text = resp.text().await.unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&body_text).unwrap();
        assert_eq!(parsed["method"], "POST");
        assert_eq!(parsed["path"], "/auth/v1/authorize");
        assert_eq!(parsed["query"], "code=abc&state=xyz");
        assert_eq!(parsed["body"], "hello rauthy");
        // Host is preserved; X-Forwarded-Proto passes through; peer IP appended
        assert_eq!(parsed["headers"]["host"], "ofm.example.com");
        assert_eq!(parsed["headers"]["x-forwarded-proto"], "https");
        let xff = parsed["headers"]["x-forwarded-for"].as_str().unwrap();
        assert!(xff.starts_with("203.0.113.9, "), "xff={xff}");
        // Hop-by-hop header must be stripped from the upstream request
        assert!(parsed["headers"].get("connection").is_none());
        // ... and from the response relayed back to the browser.
        assert!(headers_before.is_none());
    }

    #[tokio::test]
    async fn test_proxy_relays_non_2xx_status() {
        let (echo_port, _echo_handle) = spawn_echo_server().await;
        let proxy_base = spawn_proxy(echo_port).await;

        let resp = reqwest::Client::new()
            .post(format!("{proxy_base}/v1/not-found?status=404"))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), 404);
        let connection_header = resp.headers().get("connection").cloned();
        let echo_status = resp.headers().get("x-echo-status").cloned();
        let body_text = resp.text().await.unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&body_text).unwrap();
        assert_eq!(parsed["method"], "POST");
        assert_eq!(parsed["path"], "/auth/v1/not-found");
        // The echo server sets a hop-by-hop header on the response; the proxy
        // must strip it before handing the response to the browser.
        assert!(connection_header.is_none());
        assert_eq!(echo_status.unwrap(), "404");
    }

    #[test]
    fn test_hop_by_hop_covers_rauthy_relevant_headers() {
        for h in ["connection", "transfer-encoding", "upgrade", "keep-alive"] {
            assert!(HOP_BY_HOP.contains(&h), "{h} must be hop-by-hop");
        }
    }
}
