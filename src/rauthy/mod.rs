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

pub async fn start_rauthy(
    footprint: &str,
    hostname: &str,
    port: u16,
    proxy_port: u16,
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
    let client_config = build_client_config(hostname, proxy_port);
    std::fs::write(
        format!("{}/clients.json", bootstrap_dir),
        serde_json::to_string_pretty(&client_config)?,
    )?;

    let mut cmd = Command::new("docker");
    cmd.args(build_docker_run_args(
        hostname,
        port,
        &data_dir,
        &bootstrap_dir,
        &name,
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
/// the configured hostname rather than a hardcoded `localhost`/`127.0.0.1`.
fn build_client_config(hostname: &str, proxy_port: u16) -> serde_json::Value {
    serde_json::json!([{
        "id": "ofm",
        "name": "Ofm",
        "enabled": true,
        "redirect_uris": [
            format!("http://{hostname}:{}/*", proxy_port),
        ],
        "post_logout_redirect_uris": [
            format!("http://{hostname}:{}/*", proxy_port),
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
/// The `-p` binding is always `0.0.0.0`: Docker only accepts IP addresses for
/// the host bind interface, and `OFM_HOSTNAME` may be a non-IP hostname (e.g.
/// `myhost.local`), which would make `docker run` fail at startup. Binding all
/// interfaces still lets the browser reach rauthy via the configured hostname.
/// `PUB_URL` advertises the configured hostname so rauthy's OIDC discovery and
/// referral URLs point where `ofm` is actually reachable.
fn build_docker_run_args(
    hostname: &str,
    port: u16,
    data_dir: &str,
    bootstrap_dir: &str,
    container_name: &str,
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
        format!("0.0.0.0:{port}:8080"),
    ];
    #[cfg(unix)]
    {
        let (uid, gid) = host_uid_gid();
        args.push("--user".to_string());
        args.push(format!("{}:{}", uid, gid));
    }
    args.extend([
        "-e".to_string(),
        format!("PUB_URL={hostname}:{port}"),
        "-e".to_string(),
        "BOOTSTRAP_DIR=/app/bootstrap".to_string(),
        "-e".to_string(),
        "DISABLE_REFRESH_TOKEN_NBF=true".to_string(),
        "-e".to_string(),
        "LISTEN_SCHEME=http".to_string(),
        "-e".to_string(),
        "LOCAL_TEST=true".to_string(),
        RAUTHY_IMAGE.to_string(),
    ]);
    args
}

/// Polls the container's `/health` endpoint until it reports healthy.
///
/// The probe uses loopback: the container's port is published on `0.0.0.0`, so
/// `127.0.0.1` always reaches it regardless of whether `OFM_HOSTNAME` resolves
/// on the host. `pub_url` (advertised via `PUB_URL`) is what the browser-facing
/// URLs use, not the health probe.
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
    fn test_build_client_config_uses_hostname() {
        let config = super::build_client_config("192.168.1.50", 3183);
        let json = serde_json::to_string(&config).unwrap();

        assert!(
            json.contains(r#"http://192.168.1.50:3183/*"#),
            "redirect_uris should use the configured hostname, got: {json}"
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
    fn test_build_docker_run_args_uses_hostname() {
        let args = super::build_docker_run_args(
            "192.168.1.50",
            18080,
            "/fp/rauthy/data",
            "/fp/rauthy/bootstrap",
            "ofm-rauthy-abcdef0123456789",
        );
        let joined = args.join(" ");

        assert!(
            joined.contains("0.0.0.0:18080:8080"),
            "docker -p should always bind 0.0.0.0, got: {joined}"
        );
        assert!(
            joined.contains("PUB_URL=192.168.1.50:18080"),
            "PUB_URL should advertise the configured hostname, got: {joined}"
        );
        assert!(
            !joined.contains("localhost"),
            "docker args must not hardcode localhost: {joined}"
        );
        assert!(
            !joined.contains("127.0.0.1"),
            "docker args must not hardcode 127.0.0.1: {joined}"
        );
    }

    #[test]
    fn test_build_docker_run_args_binds_any_hostname() {
        let args = super::build_docker_run_args(
            "myhost.local",
            18080,
            "/fp/rauthy/data",
            "/fp/rauthy/bootstrap",
            "ofm-rauthy-abcdef0123456789",
        );
        let joined = args.join(" ");

        assert!(
            joined.contains("0.0.0.0:18080:8080"),
            "docker -p must bind an IP (0.0.0.0) even for a non-IP hostname, got: {joined}"
        );
        assert!(
            joined.contains("PUB_URL=myhost.local:18080"),
            "PUB_URL should advertise the non-IP hostname, got: {joined}"
        );
    }
}
