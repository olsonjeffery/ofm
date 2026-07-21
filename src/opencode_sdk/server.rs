use std::path::PathBuf;
use std::time::Duration;

use base64::Engine;
use tempfile::TempDir;
use uuid::Uuid;

use crate::opencode_sdk::SdkError;

/// Options for creating an opencode server.
#[derive(Debug, Clone)]
pub struct ServerOptions {
    pub hostname: String,
    pub port: u16,
    pub timeout: Duration,
    pub working_dir: Option<PathBuf>,
    pub config: Option<serde_json::Value>,
    pub password: Option<String>,
}

impl Default for ServerOptions {
    fn default() -> Self {
        Self {
            hostname: "127.0.0.1".into(),
            port: 0,
            timeout: Duration::from_secs(10),
            working_dir: None,
            config: None,
            password: None,
        }
    }
}

/// A running opencode server instance.
pub struct OpenCodeServer {
    child: std::process::Child,
    port: u16,
    hostname: String,
    password: Option<String>,
    _temp_dir: TempDir,
}

impl OpenCodeServer {
    pub fn url(&self) -> String {
        format!("http://{}:{}", self.hostname, self.port)
    }

    pub fn password(&self) -> Option<&str> {
        self.password.as_deref()
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn hostname(&self) -> &str {
        &self.hostname
    }

    pub async fn shutdown(&mut self) -> Result<bool, SdkError> {
        let pid = self.child.id();
        // Close stdin to signal the process, then kill + wait to reap it.
        let _ = self.child.stdin.take();
        let _ = self.child.kill();
        let _ = self.child.wait();

        // Port-probe to confirm the subprocess is no longer listening. This
        // is a best-effort check — the port may be released slightly after
        // wait() returns, so we use a short timeout.
        let addr = format!("{}:{}", self.hostname, self.port);
        let probe = tokio::time::timeout(
            Duration::from_millis(500),
            tokio::net::TcpStream::connect(&addr),
        )
        .await;

        match probe {
            Ok(Ok(_)) => {
                tracing::error!(
                    port = self.port,
                    pid = pid,
                    "opencode subprocess still listening after shutdown"
                );
                Ok(false)
            }
            Ok(Err(_)) => {
                tracing::info!(
                    port = self.port,
                    pid = pid,
                    "opencode subprocess confirmed dead"
                );
                Ok(true)
            }
            Err(_) => {
                tracing::warn!(
                    port = self.port,
                    pid = pid,
                    "port probe timed out — assuming dead"
                );
                Ok(true)
            }
        }
    }
}

impl Drop for OpenCodeServer {
    fn drop(&mut self) {
        let _ = self.child.stdin.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Create a new opencode server.
pub async fn create_opencode_server(options: ServerOptions) -> Result<OpenCodeServer, SdkError> {
    let temp_dir = TempDir::new().map_err(SdkError::Io)?;

    let config = match options.config {
        Some(cfg) => cfg.to_string(),
        None => {
            let default = serde_json::json!({
                "provider": {},
                "permission": {
                    "edit": "allow",
                    "bash": "allow",
                    "webfetch": "allow",
                    "doom_loop": "allow",
                    "external_directory": "allow"
                }
            });
            default.to_string()
        }
    };

    let config_path = temp_dir.path().join("opencode.json");
    std::fs::write(&config_path, &config).map_err(SdkError::Io)?;

    let port = if options.port == 0 {
        pick_free_port()?
    } else {
        options.port
    };

    let hostname = options.hostname;
    let password = options
        .password
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let health_timeout = options.timeout;

    let mut cmd = std::process::Command::new("opencode");
    cmd.arg("serve")
        .arg("--port")
        .arg(port.to_string())
        .arg("--hostname")
        .arg(&hostname)
        .env("OPENCODE_CONFIG", &config_path)
        .env("OPENCODE_SERVER_PASSWORD", &password)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::inherit());

    // Put the child in its own process group so it survives a Ctrl-C
    // delivered to the ofm process group, and so we can clean it up
    // independently.  This is a unix-only feature.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    if let Some(dir) = &options.working_dir {
        cmd.current_dir(dir);
    }
    let mut child = cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            SdkError::Protocol("opencode binary not found in PATH".to_string())
        } else {
            SdkError::Io(e)
        }
    })?;

    let base_url = format!("http://{hostname}:{port}");
    let http_client = reqwest::Client::new();
    wait_for_health(
        &http_client,
        &base_url,
        &password,
        Some(&mut child),
        health_timeout,
    )
    .await?;

    Ok(OpenCodeServer {
        child,
        port,
        hostname,
        password: Some(password),
        _temp_dir: temp_dir,
    })
}

fn basic_auth_header(password: &str) -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(format!("opencode:{password}"));
    format!("Basic {encoded}")
}

fn pick_free_port() -> Result<u16, SdkError> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(SdkError::Io)?;
    let port = listener.local_addr().map_err(SdkError::Io)?.port();
    drop(listener);
    Ok(port)
}

async fn wait_for_health(
    client: &reqwest::Client,
    base_url: &str,
    password: &str,
    mut child: Option<&mut std::process::Child>,
    timeout: Duration,
) -> Result<(), SdkError> {
    let url = format!("{base_url}/global/health");
    let max_attempts = 20;
    let interval = timeout / max_attempts;

    for i in 0..max_attempts {
        if let Some(child) = child.as_mut() {
            if let Some(status) = child.try_wait().map_err(SdkError::Io)? {
                return Err(SdkError::Protocol(format!(
                    "opencode process exited prematurely with status: {status}"
                )));
            }
        }

        match client
            .get(&url)
            .header("Authorization", basic_auth_header(password))
            .timeout(Duration::from_millis(500))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => return Ok(()),
            _ => {
                if i == max_attempts - 1 {
                    return Err(SdkError::Timeout);
                }
                tokio::time::sleep(interval).await;
            }
        }
    }
    Err(SdkError::Timeout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_opencode_sdk_pick_free_port() {
        let port = pick_free_port().unwrap();
        assert!(port > 0);
    }

    #[test]
    fn test_opencode_sdk_server_options_default() {
        let opts = ServerOptions::default();
        assert_eq!(opts.hostname, "127.0.0.1");
        assert_eq!(opts.port, 0);
        assert_eq!(opts.timeout, Duration::from_secs(10));
    }

    #[test]
    fn test_opencode_sdk_config_file_creation() {
        let temp_dir = TempDir::new().unwrap();
        let config = serde_json::json!({"provider": {}, "permission": {"edit": "allow"}});
        let config_path = temp_dir.path().join("opencode.json");
        std::fs::write(&config_path, config.to_string()).unwrap();
        let content = std::fs::read_to_string(&config_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["permission"]["edit"], "allow");
    }

    #[test]
    fn test_opencode_sdk_basic_auth_header_format() {
        let header = basic_auth_header("test-password");
        assert!(header.starts_with("Basic "));
        let encoded = header.strip_prefix("Basic ").unwrap();
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .unwrap();
        let decoded_str = String::from_utf8(decoded).unwrap();
        assert_eq!(decoded_str, "opencode:test-password");
    }

    #[test]
    fn test_opencode_sdk_default_config_format() {
        let config = serde_json::json!({
            "provider": {},
            "permission": {
                "edit": "allow",
                "bash": "allow",
                "webfetch": "allow",
                "doom_loop": "allow",
                "external_directory": "allow"
            }
        });
        assert_eq!(config["provider"], serde_json::json!({}));
        assert_eq!(config["permission"]["edit"], "allow");
    }

    #[test]
    fn test_opencode_sdk_url_construction() {
        let temp_dir = TempDir::new().unwrap();
        let child = std::process::Command::new("true").spawn().unwrap();
        let server = OpenCodeServer {
            child,
            port: 3183,
            hostname: "127.0.0.1".to_string(),
            password: Some("pw".to_string()),
            _temp_dir: temp_dir,
        };
        assert_eq!(server.url(), "http://127.0.0.1:3183");
        assert_eq!(server.password(), Some("pw"));
        assert_eq!(server.port(), 3183);
    }

    #[tokio::test]
    async fn test_opencode_sdk_shutdown_noop_child() {
        let temp_dir = TempDir::new().unwrap();
        let child = std::process::Command::new("true").spawn().unwrap();
        let mut server = OpenCodeServer {
            child,
            port: 9999,
            hostname: "127.0.0.1".to_string(),
            password: None,
            _temp_dir: temp_dir,
        };
        let result = server.shutdown().await.unwrap();
        assert!(result);
    }
}
