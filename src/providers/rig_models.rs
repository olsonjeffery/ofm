use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION};
use serde::Deserialize;

use crate::providers::rig_config::{is_safe_base_url, RigProviderConfig, RigVendor};

const LISTING_TIMEOUT: Duration = Duration::from_secs(10);

/// Test escape hatch: when set, the SSRF re-validation of `base_url` in
/// `list_models` is skipped so tests can point an `OpenAiCompatible` rig
/// provider at a loopback mock upstream. Setting an env var requires local
/// process control — the same trust level as the config files themselves — so
/// this does not weaken the SSRF threat model, which protects against
/// web-created configs pointing at internal hosts.
pub(crate) const LOOPBACK_BYPASS_ENV: &str = "OFM_RIG_MODELS_ALLOW_LOOPBACK";

#[derive(Deserialize)]
struct ModelsResponse {
    data: Vec<ModelEntry>,
}

#[derive(Deserialize)]
struct ModelEntry {
    id: String,
}

/// Resolve the URL prefix to which `/models` is appended.
fn listing_base_url(cfg: &RigProviderConfig) -> Result<String, String> {
    match cfg.vendor {
        RigVendor::Anthropic => Ok("https://api.anthropic.com/v1".into()),
        RigVendor::OpenAi => Ok("https://api.openai.com/v1".into()),
        RigVendor::OpenCodeGo => Ok("https://opencode.ai/zen/go/v1".into()),
        RigVendor::OpenRouter => Ok("https://openrouter.ai/api/v1".into()),
        RigVendor::OpenAiCompatible | RigVendor::OpenAiCompatibleNoAuth => cfg
            .base_url
            .clone()
            .filter(|u| !u.trim().is_empty())
            .ok_or_else(|| "base_url is required for this vendor's model listing".to_string()),
    }
}

fn auth_headers(cfg: &RigProviderConfig) -> Result<HeaderMap, String> {
    let mut headers = HeaderMap::new();
    match cfg.vendor {
        RigVendor::Anthropic => {
            let key = cfg
                .api_key
                .as_deref()
                .ok_or_else(|| "api_key is required for model listing".to_string())?;
            headers.insert(
                HeaderName::from_static("x-api-key"),
                HeaderValue::from_str(key).map_err(|e| e.to_string())?,
            );
            headers.insert(
                HeaderName::from_static("anthropic-version"),
                HeaderValue::from_static("2023-06-01"),
            );
        }
        RigVendor::OpenAiCompatibleNoAuth => {}
        _ => {
            let key = cfg
                .api_key
                .as_deref()
                .ok_or_else(|| "api_key is required for model listing".to_string())?;
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {key}")).map_err(|e| e.to_string())?,
            );
        }
    }
    Ok(headers)
}

/// Live model listing for a rig config. SSRF re-validates the captured
/// `base_url`, GETs `{base}/models` (base per vendor — see `listing_base_url`),
/// parses `data[].id`, and returns sorted + deduped ids.
///
/// Callers should only invoke this for `ModelListMode::OpenApiList` configs;
/// `Manual` mode exposes its list without any HTTP.
pub async fn list_models(cfg: &RigProviderConfig) -> Result<Vec<String>, String> {
    let base = listing_base_url(cfg)?;
    if std::env::var_os(LOOPBACK_BYPASS_ENV).is_none()
        && cfg
            .base_url
            .as_deref()
            .is_some_and(|bu| !is_safe_base_url(bu))
    {
        return Err("base_url is not a public http(s) endpoint".into());
    }
    list_models_from_base(cfg, &base).await
}

/// The shared HTTP engine behind `list_models`, split out as a test seam so
/// header/parsing behavior can be exercised against a loopback mock without
/// depending on vendor default endpoints being reachable.
async fn list_models_from_base(cfg: &RigProviderConfig, base: &str) -> Result<Vec<String>, String> {
    let url = format!("{base}/models");
    let client = reqwest::Client::builder()
        .timeout(LISTING_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get(&url)
        .headers(auth_headers(cfg)?)
        .send()
        .await
        .map_err(|e| format!("model listing request failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("model listing returned HTTP {}", resp.status()));
    }
    let body: ModelsResponse = resp
        .json()
        .await
        .map_err(|e| format!("unexpected model listing response: {e}"))?;
    let mut ids: Vec<String> = body
        .data
        .into_iter()
        .map(|m| m.id.trim().to_string())
        .filter(|id| !id.is_empty())
        .collect();
    ids.sort();
    ids.dedup();
    Ok(ids)
}

#[cfg(test)]
pub(crate) mod test_env {
    //! Test-only helpers for pointing rig providers at a loopback mock
    //! upstream. The production SSRF guard rejects loopback/private hosts, so
    //! tests toggle `LOOPBACK_BYPASS_ENV`. All toggling is serialized through
    //! `LOOPBACK_ENV_GUARD` so parallel unit tests in the same binary never
    //! race on the process-wide env var.
    use std::sync::LazyLock;
    use tokio::sync::Mutex;

    use super::LOOPBACK_BYPASS_ENV;

    pub(crate) static LOOPBACK_ENV_GUARD: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    /// Run `fut` with the SSRF loopback re-validation disabled.
    pub(crate) async fn allow_loopback<F, T>(fut: F) -> T
    where
        F: std::future::Future<Output = T>,
    {
        let _guard = LOOPBACK_ENV_GUARD.lock().await;
        std::env::set_var(LOOPBACK_BYPASS_ENV, "1");
        fut.await
    }

    /// Run `fut` with the SSRF loopback re-validation enforced (production
    /// default), so a test can assert a loopback `base_url` is rejected.
    pub(crate) async fn deny_loopback<F, T>(fut: F) -> T
    where
        F: std::future::Future<Output = T>,
    {
        let _guard = LOOPBACK_ENV_GUARD.lock().await;
        std::env::remove_var(LOOPBACK_BYPASS_ENV);
        fut.await
    }
}

#[cfg(test)]
mod tests {
    use super::test_env::{allow_loopback, deny_loopback};
    use super::*;
    use crate::providers::rig_config::ModelListMode;
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::sync::Mutex;

    struct MockUpstream {
        base: String,
        request_heads: Arc<Mutex<Vec<String>>>,
    }

    async fn spawn_mock(status: u16, body: &'static str) -> MockUpstream {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base = format!("http://{addr}");
        let request_heads = Arc::new(Mutex::new(Vec::new()));
        let heads = request_heads.clone();
        tokio::spawn(async move {
            loop {
                let (mut socket, _) = match listener.accept().await {
                    Ok(pair) => pair,
                    Err(_) => break,
                };
                let mut buf = vec![0u8; 16_384];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                if n == 0 {
                    continue;
                }
                heads
                    .lock()
                    .await
                    .push(String::from_utf8_lossy(&buf[..n]).to_string());
                let reason = if status == 200 { "OK" } else { "Error" };
                let resp = format!(
                    "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = socket.write_all(resp.as_bytes()).await;
            }
        });
        MockUpstream {
            base,
            request_heads,
        }
    }

    fn openai_compatible_cfg(base_url: &str) -> RigProviderConfig {
        RigProviderConfig {
            name: "mock-provider".into(),
            vendor: RigVendor::OpenAiCompatible,
            base_url: Some(base_url.into()),
            api_key: Some("sk-test".into()),
            model_list_mode: ModelListMode::OpenApiList,
            models: vec![],
        }
    }

    #[tokio::test]
    async fn test_openai_shape_sorted_deduped_with_bearer() {
        let mock = spawn_mock(200, r#"{"data":[{"id":"m-b"},{"id":"m-a"},{"id":"m-a"}]}"#).await;
        let cfg = openai_compatible_cfg(&mock.base);
        let models = allow_loopback(async { list_models(&cfg).await }).await;
        let models = models.expect("listing should succeed");
        assert_eq!(models, vec!["m-a", "m-b"]);
        let heads = mock.request_heads.lock().await;
        assert!(!heads.is_empty(), "mock should have received a request");
        let head = &heads[0];
        assert!(head.starts_with("GET /models HTTP/1.1"));
        assert!(head.contains("authorization: Bearer sk-test"));
    }

    #[tokio::test]
    async fn test_anthropic_shape_and_headers() {
        let cfg = RigProviderConfig {
            name: "anthropic".into(),
            vendor: RigVendor::Anthropic,
            base_url: None,
            api_key: Some("sk-ant-test".into()),
            model_list_mode: ModelListMode::OpenApiList,
            models: vec![],
        };
        assert_eq!(
            listing_base_url(&cfg).unwrap(),
            "https://api.anthropic.com/v1"
        );
        let hdrs = auth_headers(&cfg).unwrap();
        assert_eq!(hdrs.get("x-api-key").unwrap(), "sk-ant-test");
        assert_eq!(hdrs.get("anthropic-version").unwrap(), "2023-06-01");

        let mock = spawn_mock(200, r#"{"data":[{"id":"claude-3"}]}"#).await;
        let models = list_models_from_base(&cfg, &mock.base).await;
        let models = models.expect("anthropic shape should parse");
        assert_eq!(models, vec!["claude-3"]);
        let heads = mock.request_heads.lock().await;
        let head = &heads[0];
        assert!(head.contains("x-api-key: sk-ant-test"));
        assert!(head.contains("anthropic-version: 2023-06-01"));
    }

    #[tokio::test]
    async fn test_no_auth_sends_no_authorization_header() {
        let mock = spawn_mock(200, r#"{"data":[{"id":"m-1"}]}"#).await;
        let cfg = RigProviderConfig {
            name: "noauth".into(),
            vendor: RigVendor::OpenAiCompatibleNoAuth,
            base_url: Some(mock.base.clone()),
            api_key: None,
            model_list_mode: ModelListMode::OpenApiList,
            models: vec![],
        };
        assert_eq!(listing_base_url(&cfg).unwrap(), mock.base);
        let models = allow_loopback(async { list_models(&cfg).await }).await;
        let models = models.expect("no-auth listing should succeed");
        assert_eq!(models, vec!["m-1"]);
        let heads = mock.request_heads.lock().await;
        assert!(
            !heads[0].contains("authorization"),
            "no-auth provider must not send an Authorization header: {}",
            heads[0]
        );
    }

    #[tokio::test]
    async fn test_non_2xx_status_returns_err() {
        let mock = spawn_mock(500, r#"{"error":"boom"}"#).await;
        let cfg = openai_compatible_cfg(&mock.base);
        let err = allow_loopback(async { list_models(&cfg).await })
            .await
            .unwrap_err();
        assert!(err.contains("HTTP 500"), "got: {err}");
    }

    #[tokio::test]
    async fn test_malformed_json_returns_err() {
        let mock = spawn_mock(200, "not-json").await;
        let cfg = openai_compatible_cfg(&mock.base);
        let err = allow_loopback(async { list_models(&cfg).await })
            .await
            .unwrap_err();
        assert!(
            err.contains("unexpected model listing response"),
            "got: {err}"
        );
    }

    #[tokio::test]
    async fn test_missing_api_key_returns_err() {
        let cfg = RigProviderConfig {
            name: "nokey".into(),
            vendor: RigVendor::OpenAi,
            base_url: None,
            api_key: None,
            model_list_mode: ModelListMode::OpenApiList,
            models: vec![],
        };
        let err = list_models(&cfg).await.unwrap_err();
        assert!(err.contains("api_key is required"), "got: {err}");
    }

    #[tokio::test]
    async fn test_missing_base_url_returns_err() {
        let cfg = RigProviderConfig {
            base_url: None,
            ..openai_compatible_cfg("https://example.com")
        };
        let err = list_models(&cfg).await.unwrap_err();
        assert!(err.contains("base_url is required"), "got: {err}");
    }

    #[tokio::test]
    async fn test_unreachable_host_returns_err() {
        let cfg = openai_compatible_cfg("http://127.0.0.1:1");
        let err = allow_loopback(async { list_models(&cfg).await })
            .await
            .unwrap_err();
        assert!(err.contains("model listing request failed"), "got: {err}");
    }

    #[tokio::test]
    async fn test_redirect_not_followed() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let base = format!("http://{addr}");
        let requests = Arc::new(Mutex::new(0u32));
        let counter = requests.clone();
        let location = format!("{base}/models");
        tokio::spawn(async move {
            loop {
                let (mut socket, _) = match listener.accept().await {
                    Ok(pair) => pair,
                    Err(_) => break,
                };
                let mut buf = vec![0u8; 16_384];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                if n == 0 {
                    continue;
                }
                *counter.lock().await += 1;
                let resp = format!(
                    "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
                let _ = socket.write_all(resp.as_bytes()).await;
            }
        });
        let cfg = openai_compatible_cfg(&base);
        let err = allow_loopback(async { list_models(&cfg).await })
            .await
            .unwrap_err();
        assert!(
            err.contains("HTTP 302"),
            "redirect must surface as an error status, got: {err}"
        );
        assert_eq!(*requests.lock().await, 1, "redirect must not be followed");
    }

    #[tokio::test]
    async fn test_ssrf_loopback_base_url_rejected() {
        for bad in [
            "http://127.0.0.1:8080",
            "http://[::1]:8080",
            "http://localhost:8080",
            "http://192.168.1.10",
            "http://10.0.0.5",
            "http://169.254.169.254",
            "http://foo.internal",
            "http://myhost.local",
        ] {
            let cfg = openai_compatible_cfg(bad);
            let err = deny_loopback(async { list_models(&cfg).await })
                .await
                .unwrap_err();
            assert!(
                err.contains("not a public http(s) endpoint"),
                "expected SSRF rejection for {bad}, got: {err}"
            );
        }
    }
}
