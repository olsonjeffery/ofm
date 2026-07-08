use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;

const RAUTHY_IMAGE: &str = "ghcr.io/sebadob/rauthy:latest";
const CONTAINER_NAME: &str = "omprint-rauthy";
const HEALTH_POLL_INTERVAL: Duration = Duration::from_millis(500);
const HEALTH_TIMEOUT: Duration = Duration::from_secs(120);

type BoxError = Box<dyn std::error::Error>;

pub struct RauthyInstance {
    pub port: u16,
}

impl RauthyInstance {
    pub fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for RauthyInstance {
    fn drop(&mut self) {
        let _ = std::process::Command::new("docker")
            .args(["kill", CONTAINER_NAME])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

pub fn find_available_port() -> std::io::Result<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    Ok(addr.port())
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

    let status = Command::new("docker")
        .args([
            "run",
            "-d",
            "--rm",
            "--name",
            CONTAINER_NAME,
            "-v",
            &format!("{}:/app/data", data_dir),
            "-p",
            &format!("{}:8080", port),
            "-e",
            &format!("PUBLIC_URL=http://localhost:{}/auth", proxy_port),
            "-e",
            "LOCAL_TEST=true",
            RAUTHY_IMAGE,
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await?;

    if !status.success() {
        return Err("docker run for rauthy failed".into());
    }

    Ok(RauthyInstance { port })
}

pub async fn wait_until_healthy(port: u16) -> Result<(), BoxError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;
    let start = std::time::Instant::now();

    loop {
        if start.elapsed() > HEALTH_TIMEOUT {
            let logs = Command::new("docker")
                .args(["logs", CONTAINER_NAME, "--tail", "20"])
                .output()
                .await
                .ok();
            if let Some(output) = logs {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let stdout = String::from_utf8_lossy(&output.stdout);
                tracing::error!("rauthy container logs (stdout): {stdout}");
                tracing::error!("rauthy container logs (stderr): {stderr}");
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

pub async fn stop_rauthy() {
    let _ = Command::new("docker")
        .args(["kill", CONTAINER_NAME])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;
}
