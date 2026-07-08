use std::process::Stdio;
use std::time::Duration;

use tokio::io::AsyncBufReadExt;
use tokio::process::{Child, Command};

const RAUTHY_IMAGE: &str = "ghcr.io/sebadob/rauthy:latest";
const CONTAINER_NAME: &str = "omprint-rauthy";
const HEALTH_POLL_INTERVAL: Duration = Duration::from_millis(500);
const HEALTH_TIMEOUT: Duration = Duration::from_secs(120);

type BoxError = Box<dyn std::error::Error>;

pub struct RauthyInstance {
    pub port: u16,
    child: Option<Child>,
}

impl RauthyInstance {
    pub fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for RauthyInstance {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.start_kill();
        }
    }
}

pub fn find_available_port() -> std::io::Result<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    Ok(addr.port())
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
    port: u16,
    proxy_port: u16,
) -> Result<RauthyInstance, BoxError> {
    tokio::process::Command::new("docker")
        .args(["rm", "-f", CONTAINER_NAME])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .ok();

    let data_dir = format!("{}/rauthy/data", footprint);
    std::fs::create_dir_all(&data_dir)?;

    #[cfg(unix)]
    let user_flag: Option<String> = {
        let uid = std::process::Command::new("id")
            .arg("-u")
            .output()
            .ok()
            .and_then(|o| {
                if o.status.success() {
                    String::from_utf8(o.stdout)
                        .ok()
                        .map(|s| s.trim().to_string())
                } else {
                    None
                }
            });
        let gid = std::process::Command::new("id")
            .arg("-g")
            .output()
            .ok()
            .and_then(|o| {
                if o.status.success() {
                    String::from_utf8(o.stdout)
                        .ok()
                        .map(|s| s.trim().to_string())
                } else {
                    None
                }
            });
        uid.zip(gid).map(|(u, g)| format!("{}:{}", u, g))
    };
    #[cfg(not(unix))]
    let user_flag: Option<String> = None;

    let mut cmd = Command::new("docker");
    cmd.args(["run", "--rm", "--name", CONTAINER_NAME]);
    if let Some(ref u) = user_flag {
        cmd.args(["-u", u]);
    }
    cmd.arg("-v");
    cmd.arg(format!("{}:/app/data", data_dir));
    cmd.arg("-p");
    cmd.arg(format!("{}:8080", port));
    cmd.arg("-e");
    cmd.arg(format!("PUBLIC_URL=http://localhost:{}/auth", proxy_port));
    cmd.arg("-e");
    cmd.arg("LOCAL_TEST=true");
    cmd.arg(RAUTHY_IMAGE);

    let mut child = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;

    if let Some(stdout) = child.stdout.take() {
        spawn_reader(stdout, "rauthy");
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_reader(stderr, "rauthy");
    }

    Ok(RauthyInstance {
        port,
        child: Some(child),
    })
}

pub async fn wait_until_healthy(port: u16) -> Result<(), BoxError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;
    let start = std::time::Instant::now();

    loop {
        if start.elapsed() > HEALTH_TIMEOUT {
            let logs = Command::new("docker")
                .args(["logs", CONTAINER_NAME, "--tail", "50"])
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
            .get(format!("http://127.0.0.1:{}/health", port))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => return Ok(()),
            _ => tokio::time::sleep(HEALTH_POLL_INTERVAL).await,
        }
    }
}
