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
//!   outbound request and the inbound response, as is the RFC 7239 `Forwarded`
//!   header (rauthy prefers its `Forwarded: for=...` value over `X-Forwarded-For`
//!   when present, so forwarding a client-supplied value would let a client
//!   spoof the source IP rauthy sees).
//! - The original incoming `Host` header is preserved (rauthy uses it for
//!   cookie domain selection).
//! - `X-Forwarded-Host` / `X-Forwarded-Proto` are derived from the configured
//!   `pub_url` and overwritten on every request. Client-supplied values are
//!   never trusted: rauthy (in `proxy_mode`) trusts its proxies'
//!   `X-Forwarded-*`, so forwarding client values would let a client dictate
//!   the host/scheme rauthy uses to build absolute URLs.
//! - `X-Forwarded-For` is overwritten with the direct peer IP so rauthy (in
//!   `proxy_mode`) never trusts a client-supplied chain. An incoming chain is
//!   preserved only when the direct peer is itself a proxy the operator listed
//!   in `OFM_RAUTHY_TRUSTED_PROXIES` (e.g. OFM behind an external reverse
//!   proxy); the peer IP is then appended.

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
    "forwarded",
];

const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;

/// Upstream target + forwarding policy for the rauthy proxy, shared across
/// requests.
#[derive(Clone)]
struct ProxyConfig {
    client: ProxyClient,
    rauthy_port: u16,
    forwarded_host: String,
    forwarded_proto: String,
    /// CIDRs OFM trusts as upstream proxies (from `OFM_RAUTHY_TRUSTED_PROXIES`).
    /// A request whose direct peer falls in one of these may keep its incoming
    /// `X-Forwarded-For` chain; any other peer is rewritten to just the peer IP.
    trusted: Vec<ipnet::IpNet>,
}

/// The public `scheme` and `host[:port]` derived from the configured `pub_url`,
/// sent to rauthy as `X-Forwarded-Proto` / `X-Forwarded-Host`. Rauthy's
/// `PUB_URL` env is the `host[:port]` portion of the same `pub_url`, so rauthy
/// always sees a forwarded origin that matches the origin it advertises.
fn forwarded_origin(pub_url: &str) -> (String, String) {
    let scheme = url::Url::parse(&crate::rauthy::with_http_scheme(pub_url))
        .map(|u| u.scheme().to_string())
        .unwrap_or_else(|_| "http".to_string());
    (crate::rauthy::pub_url_host_port(pub_url), scheme)
}

/// Build a standalone `/auth` router that proxies to the rauthy container on
/// `rauthy_port`. Returns a `Router<()>` so it can be nested into any state.
pub fn rauthy_proxy_router(
    rauthy_port: u16,
    pub_url: &str,
    trusted_proxies: Option<&str>,
) -> Router {
    let client = hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
        .build(hyper_util::client::legacy::connect::HttpConnector::new());
    let (forwarded_host, forwarded_proto) = forwarded_origin(pub_url);
    let trusted = trusted_proxies
        .into_iter()
        .flat_map(|list| list.lines().map(str::trim))
        .filter_map(|line| line.parse::<ipnet::IpNet>().ok())
        .collect();
    Router::new()
        .fallback(proxy_handler)
        .with_state(ProxyConfig {
            client,
            rauthy_port,
            forwarded_host,
            forwarded_proto,
            trusted,
        })
}

fn bad_gateway(msg: &'static str) -> Response<Body> {
    Response::builder()
        .status(StatusCode::BAD_GATEWAY)
        .body(Body::from(msg))
        .unwrap()
}

async fn proxy_handler(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    State(config): State<ProxyConfig>,
    req: Request<Body>,
) -> Response<Body> {
    let (mut parts, body) = req.into_parts();

    let body_bytes = match axum::body::to_bytes(body, MAX_BODY_BYTES).await {
        Ok(bytes) => bytes,
        Err(e) => {
            tracing::warn!(error = %e, "rauthy proxy: failed to buffer request body");
            return bad_gateway("failed to buffer request body");
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
    parts.uri = match format!(
        "http://127.0.0.1:{}/auth{path_and_query}",
        config.rauthy_port
    )
    .parse::<Uri>()
    {
        Ok(uri) => uri,
        Err(e) => {
            tracing::warn!(error = %e, "rauthy proxy: invalid target URI");
            return bad_gateway("invalid target URI");
        }
    };

    // Strip hop-by-hop headers (incl. `Forwarded`). X-Forwarded-Host/Proto are
    // always overwritten with the configured public origin (never forwarded
    // from the client). X-Forwarded-For is set to the direct peer IP — a
    // client-supplied chain is only preserved when the peer is a trusted
    // upstream proxy, and the peer IP is then appended.
    let mut headers = std::mem::take(&mut parts.headers);
    for name in HOP_BY_HOP {
        headers.remove(*name);
    }
    headers.insert(
        "x-forwarded-host",
        HeaderValue::from_str(&config.forwarded_host)
            .unwrap_or_else(|_| HeaderValue::from_static("")),
    );
    headers.insert(
        "x-forwarded-proto",
        HeaderValue::from_str(&config.forwarded_proto)
            .unwrap_or_else(|_| HeaderValue::from_static("")),
    );
    let peer_trusted = config.trusted.iter().any(|net| net.contains(&peer.ip()));
    let xff_value = if peer_trusted {
        match headers.get("x-forwarded-for").and_then(|h| h.to_str().ok()) {
            Some(existing) if !existing.trim().is_empty() => {
                format!("{existing}, {}", peer.ip())
            }
            _ => peer.ip().to_string(),
        }
    } else {
        peer.ip().to_string()
    };
    headers.insert(
        "x-forwarded-for",
        HeaderValue::from_str(&xff_value).unwrap_or_else(|_| HeaderValue::from_static("")),
    );
    parts.headers = headers;

    let request = Request::from_parts(parts, Body::from(body_bytes));

    let resp = match config.client.request(request).await {
        Ok(resp) => resp,
        Err(e) => {
            tracing::warn!(error = %e, "rauthy proxy: upstream request failed");
            return bad_gateway("upstream request failed");
        }
    };

    let (resp_parts, resp_body) = resp.into_parts();
    let mut headers = resp_parts.headers;
    for name in HOP_BY_HOP {
        headers.remove(*name);
    }
    let bytes = match resp_body.collect().await.map(|c| c.to_bytes()) {
        Ok(bytes) => bytes,
        Err(e) => {
            tracing::warn!(error = %e, "rauthy proxy: failed to read response body");
            return bad_gateway("failed to read response body");
        }
    };

    Response::builder()
        .status(resp_parts.status)
        .version(resp_parts.version)
        .body(Body::from(bytes))
        .map(|mut resp| {
            *resp.headers_mut() = headers;
            resp
        })
        .unwrap_or_else(|_| bad_gateway("failed to build response"))
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
    async fn spawn_proxy(rauthy_port: u16, pub_url: &str, trusted: Option<&str>) -> String {
        let app = rauthy_proxy_router(rauthy_port, pub_url, trusted);
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
        let proxy_base = spawn_proxy(echo_port, "https://ofm.example.com", None).await;

        let resp = reqwest::Client::new()
            .post(format!("{proxy_base}/v1/authorize?code=abc&state=xyz"))
            .header("host", "ofm.example.com")
            // Client-supplied X-Forwarded-* must be ignored: rauthy (in
            // proxy_mode) trusts its proxy, so forwarding client values would
            // let a client dictate the origin rauthy builds URLs with — and a
            // client-supplied XFF/Forwarded chain would let it spoof the source
            // IP rauthy uses for brute-force throttling.
            .header("x-forwarded-proto", "http")
            .header("x-forwarded-host", "evil.example.com")
            .header("x-forwarded-for", "203.0.113.9")
            .header("forwarded", "for=203.0.113.9;proto=http")
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
        // Host is preserved; X-Forwarded-* derive from the configured pub_url,
        // not the client; X-Forwarded-For is the direct peer only (the client
        // chain and the Forwarded header are dropped).
        assert_eq!(parsed["headers"]["host"], "ofm.example.com");
        assert_eq!(parsed["headers"]["x-forwarded-host"], "ofm.example.com");
        assert_eq!(parsed["headers"]["x-forwarded-proto"], "https");
        assert_eq!(parsed["headers"]["x-forwarded-for"], "127.0.0.1");
        assert!(parsed["headers"].get("forwarded").is_none());
        // Hop-by-hop header must be stripped from the upstream request
        assert!(parsed["headers"].get("connection").is_none());
        // ... and from the response relayed back to the browser.
        assert!(headers_before.is_none());
    }

    #[tokio::test]
    async fn test_proxy_preserves_chain_from_trusted_proxy() {
        let (echo_port, _echo_handle) = spawn_echo_server().await;
        // A request whose direct peer is a listed trusted proxy keeps its
        // incoming X-Forwarded-For chain, with the peer IP appended.
        let proxy_base =
            spawn_proxy(echo_port, "https://ofm.example.com", Some("127.0.0.1/32")).await;

        let resp = reqwest::Client::new()
            .post(format!("{proxy_base}/v1/authorize"))
            .header("x-forwarded-for", "203.0.113.9, 198.51.100.7")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), 200);
        let body_text = resp.text().await.unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&body_text).unwrap();
        let xff = parsed["headers"]["x-forwarded-for"].as_str().unwrap();
        assert_eq!(xff, "203.0.113.9, 198.51.100.7, 127.0.0.1", "xff={xff}");
    }

    #[tokio::test]
    async fn test_proxy_relays_non_2xx_status() {
        let (echo_port, _echo_handle) = spawn_echo_server().await;
        let proxy_base = spawn_proxy(echo_port, "http://127.0.0.1:3258", None).await;

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
        for h in [
            "connection",
            "transfer-encoding",
            "upgrade",
            "keep-alive",
            "forwarded",
        ] {
            assert!(HOP_BY_HOP.contains(&h), "{h} must be hop-by-hop");
        }
    }

    #[test]
    fn test_forwarded_origin_derived_from_pub_url() {
        assert_eq!(
            super::forwarded_origin("http://127.0.0.1:3258"),
            ("127.0.0.1:3258".to_string(), "http".to_string())
        );
        assert_eq!(
            super::forwarded_origin("https://ofm.example.com"),
            ("ofm.example.com".to_string(), "https".to_string())
        );
        // Scheme-less input is treated as http (matches pub_url_host_port).
        assert_eq!(
            super::forwarded_origin("myhost.local:18080"),
            ("myhost.local:18080".to_string(), "http".to_string())
        );
    }
}
