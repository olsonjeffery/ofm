#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::AsyncBufReadExt;
use tokio::process::{Child, Command};

const RAUTHY_IMAGE: &str = "ghcr.io/sebadob/rauthy:latest";
const HEALTH_POLL_INTERVAL: Duration = Duration::from_millis(500);
const HEALTH_TIMEOUT: Duration = Duration::from_secs(120);

/// Deterministic, footprint-unique container name. Stable across restarts
/// of the same footprint so a stale container (e.g., left by a SIGKILLed
/// instance) is reaped by the startup `docker rm -f` below; unique across
/// worktree footprints so concurrent ofm instances do not collide.
///
/// Deliberately not `DefaultHasher` (stability is unspecified); this is a
/// self-contained FNV-1a 64-bit hash.
fn container_name(footprint: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325; // FNV-1a 64-bit offset basis
    for b in footprint.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("ofm-rauthy-{hash:016x}")
}

type BoxError = Box<dyn std::error::Error>;

pub struct RauthyInstance {
    port: u16,
    container_name: String,
    child: Option<Child>,
}

impl RauthyInstance {
    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn container_name(&self) -> &str {
        &self.container_name
    }
}

impl Drop for RauthyInstance {
    fn drop(&mut self) {
        // SIGKILLing the `docker run` CLI alone leaves the container
        // running. Remove our named container precisely — `--rm` on the
        // run command only fires after the container's own process exits.
        let _ = std::process::Command::new("docker")
            .args(["rm", "-f", &self.container_name])
            .status();
        if let Some(mut child) = self.child.take() {
            let _ = child.start_kill();
            // `wait()` is async and cannot be awaited from `Drop`; dropping
            // the handle is fine — init reaps the SIGKILLed CLI.
            std::mem::drop(child);
        }
    }
}

pub fn find_available_port() -> std::io::Result<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    Ok(addr.port())
}

#[cfg(unix)]
fn host_uid_gid() -> (u32, u32) {
    let parse_id = |arg: &str| -> u32 {
        std::process::Command::new("id")
            .arg(arg)
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(10001)
    };
    (parse_id("-u"), parse_id("-g"))
}

fn spawn_reader(reader: impl tokio::io::AsyncRead + Unpin + Send + 'static, label: &'static str) {
    tokio::spawn(async move {
        let mut lines = tokio::io::BufReader::new(reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            tracing::info!("[{label}] {line}");
        }
    });
}

/// Extract the `host[:port]` portion of a public URL for rauthy's `PUB_URL`
/// env (a single scalar `host[:port]`; the scheme is set via `LISTEN_SCHEME`
/// + forwarded `X-Forwarded-Proto`). Scheme-less inputs are treated as `http`.
///
/// - `"http://127.0.0.1:3258"` → `"127.0.0.1:3258"`
/// - `"https://ofm.example.com"` → `"ofm.example.com"` (well-known default port
///   is elided)
/// - `"http://localhost:8080/"` → `"localhost:8080"`
/// - `"ofm.example.com:443"` → `"ofm.example.com:443"` (scheme-less input keeps
///   its explicit port)
pub fn pub_url_host_port(pub_url: &str) -> String {
    let input = if pub_url.contains("://") {
        pub_url.to_string()
    } else {
        format!("http://{pub_url}")
    };
    let Ok(url) = url::Url::parse(&input) else {
        return pub_url.trim_end_matches('/').to_string();
    };
    let host = url.host_str().unwrap_or("localhost").to_string();
    match url.port() {
        Some(p) => format!("{host}:{p}"),
        None => host,
    }
}

/// Redirect URI wildcard for a public URL, e.g. `"http://127.0.0.1:3258/*"`.
/// Used for both `redirect_uris` and `post_logout_redirect_uris` in rauthy's
/// bootstrap `clients.json`.
pub fn client_redirect_uri(pub_url: &str) -> String {
    format!("{}/*", pub_url.trim_end_matches('/'))
}

/// Re-host an absolute URL (e.g. an endpoint from rauthy's OIDC discovery) onto
/// a different base origin, keeping the path and query. The path is taken from
/// the parsed URL; scheme-less input is treated as `http` (matching
/// `pub_url_host_port`). If the input cannot be parsed as a URL it is appended
/// verbatim to the base.
///
/// OFM never hands rauthy's self-reported endpoints to the browser verbatim:
/// rauthy builds them from its own `PUB_URL` + `LISTEN_SCHEME`, and in default
/// (non-`proxy_mode`) mode it always emits `http://` URLs — even for an
/// `https://` `pub_url` — so the discovery response is used only for the path
/// layout while the origin always comes from OFM's configured `pub_url`.
///
/// - `("http://127.0.0.1:3183/auth/v1/oidc/authorize", "https://ofm.example.com:3184")`
///   → `"https://ofm.example.com:3184/auth/v1/oidc/authorize"`
/// - `("http://127.0.0.1:3183/auth/v1/token?x=1", "http://127.0.0.1:18080")`
///   → `"http://127.0.0.1:18080/auth/v1/token?x=1"`
pub fn rehost_endpoint(url: &str, base: &str) -> String {
    let input = if url.contains("://") {
        url.to_string()
    } else {
        format!("http://{url}")
    };
    let base = base.trim_end_matches('/');
    match url::Url::parse(&input) {
        Ok(parsed) => match parsed.query() {
            Some(q) => format!("{}{}?{}", base, parsed.path(), q),
            None => format!("{}{}", base, parsed.path()),
        },
        Err(_) => format!("{}/{}", base, url.trim_start_matches('/')),
    }
}

pub async fn start_rauthy(
    footprint: &str,
    pub_url: &str,
    port: u16,
    proxy_mode: bool,
    trusted_proxies: Option<String>,
) -> Result<RauthyInstance, BoxError> {
    let name = container_name(footprint);
    // Reap any stale container left behind by a previously SIGKILLed ofm
    // instance for this footprint. The footprint-derived name is stable, so
    // this precisely targets only our own leftovers.
    tokio::process::Command::new("docker")
        .args(["rm", "-f", &name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .ok();

    let data_dir = format!("{}/rauthy/data", footprint);
    std::fs::create_dir_all(&data_dir)?;
    // Rauthy runs as the host user's UID via docker --user flag on Unix (so
    // files written to the mounted volume are owned by the host user). On
    // Windows the --user flag is omitted. Ensure the mounted data volume is
    // writable regardless of UID.
    #[cfg(unix)]
    std::fs::set_permissions(&data_dir, std::fs::Permissions::from_mode(0o777))?;

    let bootstrap_dir = format!("{}/rauthy/bootstrap", footprint);
    std::fs::create_dir_all(&bootstrap_dir)?;
    let client_config = build_client_config(pub_url);
    std::fs::write(
        format!("{}/clients.json", bootstrap_dir),
        serde_json::to_string_pretty(&client_config)?,
    )?;

    let mut cmd = Command::new("docker");
    cmd.args(build_docker_run_args(
        pub_url,
        port,
        &data_dir,
        &bootstrap_dir,
        &name,
        proxy_mode,
        trusted_proxies.as_deref(),
    ));

    let mut child = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;

    if let Some(stdout) = child.stdout.take() {
        spawn_reader(stdout, "rauthy");
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_reader(stderr, "rauthy");
    }

    Ok(RauthyInstance {
        port,
        container_name: name,
        child: Some(child),
    })
}

/// Bootstrap `clients.json` for the rauthy container. The OAuth callback /
/// logout targets must match where `ofm` is actually reachable, so they use
/// the configured `pub_url` (trimmed of trailing slash) rather than a
/// hardcoded `localhost`/`127.0.0.1`.
fn build_client_config(pub_url: &str) -> serde_json::Value {
    let redirect_uri = client_redirect_uri(pub_url);
    serde_json::json!([{
        "id": "ofm",
        "name": "Ofm",
        "enabled": true,
        "redirect_uris": [
            redirect_uri,
        ],
        "post_logout_redirect_uris": [
            redirect_uri,
        ],
        "flows_enabled": ["authorization_code", "refresh_token"],
        "access_token_alg": "EdDSA",
        "id_token_alg": "EdDSA",
        "auth_code_lifetime": 300,
        "access_token_lifetime": 1800,
        "scopes": ["openid", "profile", "email"],
        "default_scopes": ["openid", "profile", "email"],
        "challenges": ["S256"],
        "force_mfa": false,
    }])
}

/// `docker run` argument list for the rauthy container. Pure so the docker
/// invocation can be unit-tested without running Docker.
///
/// The `-p` binding is always `127.0.0.1` (loopback): the browser reaches
/// rauthy exclusively through OFM's `/auth` reverse proxy, so the published
/// port must not be reachable from the network. (Docker only accepts IP
/// addresses for the host bind interface; `127.0.0.1` works regardless of
/// whether the `pub_url` host is an IP or a hostname.) `PUB_URL` advertises
/// the `host[:port]` of OFM's `pub_url` so rauthy's OIDC discovery and
/// referral URLs point at the origin OFM serves on.
fn build_docker_run_args(
    pub_url: &str,
    port: u16,
    data_dir: &str,
    bootstrap_dir: &str,
    container_name: &str,
    proxy_mode: bool,
    trusted_proxies: Option<&str>,
) -> Vec<String> {
    let mut args = vec![
        "run".to_string(),
        "--rm".to_string(),
        "--name".to_string(),
        container_name.to_string(),
        "-v".to_string(),
        format!("{}:/app/data", data_dir),
        "-v".to_string(),
        format!("{}:/app/bootstrap", bootstrap_dir),
        "-p".to_string(),
        format!("127.0.0.1:{port}:8080"),
    ];
    #[cfg(unix)]
    {
        let (uid, gid) = host_uid_gid();
        args.push("--user".to_string());
        args.push(format!("{}:{}", uid, gid));
    }
    args.extend([
        "-e".to_string(),
        format!("PUB_URL={}", pub_url_host_port(pub_url)),
        "-e".to_string(),
        "BOOTSTRAP_DIR=/app/bootstrap".to_string(),
        "-e".to_string(),
        "DISABLE_REFRESH_TOKEN_NBF=true".to_string(),
        "-e".to_string(),
        "LISTEN_SCHEME=http".to_string(),
        "-e".to_string(),
        "LOCAL_TEST=true".to_string(),
    ]);
    if proxy_mode {
        args.extend([
            "-e".to_string(),
            "PROXY_MODE=true".to_string(),
            "-e".to_string(),
            format!(
                "TRUSTED_PROXIES={}",
                trusted_proxies.unwrap_or("127.0.0.1/32")
            ),
        ]);
    }
    args.push(RAUTHY_IMAGE.to_string());
    args
}

/// Polls the container's `/health` endpoint until it reports healthy.
///
/// The probe uses loopback: the container's port is published on `127.0.0.1`,
/// so `127.0.0.1` always reaches it regardless of whether `OFM_HOSTNAME`
/// resolves on the host. `pub_url` (advertised via `PUB_URL`) is what the
/// browser-facing URLs use, not the health probe.
pub async fn wait_until_healthy(port: u16, container_name: &str) -> Result<(), BoxError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;
    let start = std::time::Instant::now();

    loop {
        if start.elapsed() > HEALTH_TIMEOUT {
            let logs = Command::new("docker")
                .args(["logs", container_name, "--tail", "50"])
                .output()
                .await
                .ok();
            if let Some(output) = logs {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                if !stdout.is_empty() {
                    tracing::error!("rauthy container stdout:\n{stdout}");
                }
                if !stderr.is_empty() {
                    tracing::error!("rauthy container stderr:\n{stderr}");
                }
            }
            return Err("rauthy health check timed out".into());
        }

        match client
            .get(format!("http://127.0.0.1:{port}/health"))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => return Ok(()),
            _ => tokio::time::sleep(HEALTH_POLL_INTERVAL).await,
        }
    }
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    #[test]
    fn test_host_uid_gid_matches_current_user() {
        let (uid, gid) = super::host_uid_gid();

        let actual_uid: u32 = std::process::Command::new("id")
            .arg("-u")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| s.trim().parse().ok())
            .expect("failed to get actual UID");

        let actual_gid: u32 = std::process::Command::new("id")
            .arg("-g")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| s.trim().parse().ok())
            .expect("failed to get actual GID");

        assert_eq!(uid, actual_uid, "UID should match current user");
        assert_eq!(gid, actual_gid, "GID should match current user");
        assert_ne!(uid, 0, "should not run tests as root");
    }

    #[test]
    fn test_container_name_deterministic_per_footprint() {
        let fp_a = "/home/test/worktrees/project-1";
        let fp_b = "/home/test/worktrees/project-2";

        // Same footprint → identical name; stable across calls.
        let name_a1 = super::container_name(fp_a);
        let name_a2 = super::container_name(fp_a);
        assert_eq!(name_a1, name_a2);

        // Different footprints → different names.
        let name_b = super::container_name(fp_b);
        assert_ne!(name_a1, name_b);

        // Names are prefixed and hex-suffixed.
        for name in [&name_a1, &name_b] {
            assert!(name.starts_with("ofm-rauthy-"), "unexpected name: {name}");
            let suffix = name.trim_start_matches("ofm-rauthy-");
            assert_eq!(suffix.len(), 16, "expected 16 hex chars in {name}");
            assert!(
                u64::from_str_radix(suffix, 16).is_ok(),
                "suffix not hex in {name}"
            );
        }
    }

    #[test]
    fn test_build_client_config_uses_pub_url() {
        let config = super::build_client_config("http://192.168.1.50:3183");
        let json = serde_json::to_string(&config).unwrap();

        assert!(
            json.contains(r#"http://192.168.1.50:3183/*"#),
            "redirect_uris should use the configured pub_url, got: {json}"
        );
        assert!(
            !json.contains("127.0.0.1"),
            "client config must not hardcode 127.0.0.1: {json}"
        );
        assert!(
            !json.contains("localhost"),
            "client config must not hardcode localhost: {json}"
        );
        assert_eq!(
            config[0]["redirect_uris"][0].as_str(),
            Some("http://192.168.1.50:3183/*")
        );
        assert_eq!(
            config[0]["post_logout_redirect_uris"][0].as_str(),
            Some("http://192.168.1.50:3183/*")
        );
    }

    #[test]
    fn test_pub_url_host_port() {
        assert_eq!(
            super::pub_url_host_port("http://127.0.0.1:3258"),
            "127.0.0.1:3258"
        );
        assert_eq!(
            super::pub_url_host_port("https://ofm.example.com"),
            "ofm.example.com"
        );
        assert_eq!(
            super::pub_url_host_port("http://localhost:8080/"),
            "localhost:8080"
        );
        assert_eq!(
            super::pub_url_host_port("ofm.example.com:443"),
            "ofm.example.com:443"
        );
    }

    #[test]
    fn test_client_redirect_uri() {
        assert_eq!(
            super::client_redirect_uri("http://127.0.0.1:3258"),
            "http://127.0.0.1:3258/*"
        );
        assert_eq!(
            super::client_redirect_uri("https://ofm.example.com/"),
            "https://ofm.example.com/*"
        );
    }

    #[test]
    fn test_rehost_endpoint_https_pub_url() {
        // rauthy (default mode) advertises an `http://` authorization endpoint
        // for an `https://` pub_url; re-hosting must fix scheme + host while
        // keeping the path.
        assert_eq!(
            super::rehost_endpoint(
                "http://127.0.0.1:3183/auth/v1/oidc/authorize",
                "https://htpc3090.tail34e6bf.ts.net:3184",
            ),
            "https://htpc3090.tail34e6bf.ts.net:3184/auth/v1/oidc/authorize"
        );
    }

    #[test]
    fn test_rehost_endpoint_loopback_direct() {
        // Server-side endpoints are re-hosted onto the direct loopback base.
        assert_eq!(
            super::rehost_endpoint(
                "http://127.0.0.1:3183/auth/v1/token",
                "http://127.0.0.1:18080",
            ),
            "http://127.0.0.1:18080/auth/v1/token"
        );
    }

    #[test]
    fn test_rehost_endpoint_preserves_query_and_default_port() {
        assert_eq!(
            super::rehost_endpoint(
                "http://localhost:8080/auth/v1/authorize?foo=1&bar=2",
                "https://ofm.example.com",
            ),
            "https://ofm.example.com/auth/v1/authorize?foo=1&bar=2"
        );
        assert_eq!(
            super::rehost_endpoint(
                "http://localhost:8080/auth/v1/logout",
                "https://ofm.example.com",
            ),
            "https://ofm.example.com/auth/v1/logout"
        );
    }

    #[test]
    fn test_rehost_endpoint_trailing_slash_bases() {
        // Trailing slashes on either input must not double up.
        assert_eq!(
            super::rehost_endpoint("http://127.0.0.1:3183/auth/v1/", "https://ofm.example.com/",),
            "https://ofm.example.com/auth/v1/"
        );
    }

    #[test]
    fn test_rehost_endpoint_schemeless_input() {
        // Scheme-less endpoint input is treated as http and re-hosted.
        assert_eq!(
            super::rehost_endpoint("127.0.0.1:3183/auth/v1/end", "https://ofm.example.com"),
            "https://ofm.example.com/auth/v1/end"
        );
    }

    #[test]
    fn test_build_docker_run_args_uses_pub_url() {
        let args = super::build_docker_run_args(
            "http://192.168.1.50:18080",
            18080,
            "/fp/rauthy/data",
            "/fp/rauthy/bootstrap",
            "ofm-rauthy-abcdef0123456789",
            false,
            None,
        );
        let joined = args.join(" ");

        assert!(
            joined.contains("127.0.0.1:18080:8080"),
            "docker -p should bind loopback, got: {joined}"
        );
        assert!(
            joined.contains("PUB_URL=192.168.1.50:18080"),
            "PUB_URL should advertise the pub_url host:port, got: {joined}"
        );
        assert!(
            !joined.contains("localhost"),
            "docker args must not hardcode localhost: {joined}"
        );
        assert!(
            !joined.contains("0.0.0.0"),
            "docker -p must not bind all interfaces: {joined}"
        );
        assert!(
            !joined.contains("PROXY_MODE"),
            "PROXY_MODE must be absent when proxy_mode is false: {joined}"
        );
    }

    #[test]
    fn test_build_docker_run_args_binds_loopback() {
        let args = super::build_docker_run_args(
            "http://myhost.local:18080",
            18080,
            "/fp/rauthy/data",
            "/fp/rauthy/bootstrap",
            "ofm-rauthy-abcdef0123456789",
            false,
            None,
        );
        let joined = args.join(" ");

        assert!(
            joined.contains("127.0.0.1:18080:8080"),
            "docker -p must bind loopback even for a non-IP hostname, got: {joined}"
        );
        assert!(
            joined.contains("PUB_URL=myhost.local:18080"),
            "PUB_URL should advertise the non-IP hostname, got: {joined}"
        );
    }

    #[test]
    fn test_build_docker_run_args_proxy_mode() {
        let args = super::build_docker_run_args(
            "http://myhost.local:18080",
            18080,
            "/fp/rauthy/data",
            "/fp/rauthy/bootstrap",
            "ofm-rauthy-abcdef0123456789",
            true,
            Some("10.0.0.0/8\n127.0.0.1/32"),
        );
        let joined = args.join(" ");

        assert!(
            joined.contains("PROXY_MODE=true"),
            "PROXY_MODE=true must be set when proxy_mode is true: {joined}"
        );
        assert!(
            joined.contains("TRUSTED_PROXIES=10.0.0.0/8"),
            "TRUSTED_PROXIES should be passed through: {joined}"
        );
    }
}
