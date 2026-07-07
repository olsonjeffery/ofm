use std::process::Stdio;
use std::time::Duration;

use tokio::process::{Child, Command};

const RAUTHY_IMAGE: &str = "ghcr.io/sebadob/rauthy:latest";
const HEALTH_POLL_INTERVAL: Duration = Duration::from_millis(500);
const HEALTH_TIMEOUT: Duration = Duration::from_secs(60);

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

pub async fn start_rauthy(footprint: &str, port: u16) -> Result<RauthyInstance, BoxError> {
    let data_dir = format!("{}/rauthy/data", footprint);
    std::fs::create_dir_all(&data_dir)?;

    let _ = Command::new("docker")
        .args(["rm", "-f", "omprint-rauthy"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .await;

    let child = Command::new("docker")
        .args([
            "run",
            "--rm",
            "--name",
            "omprint-rauthy",
            "-v",
            &format!("{}:/app/data", data_dir),
            "-p",
            &format!("{}:8080", port),
            "-e",
            "PUBLIC_URL=http://localhost:PORT/auth",
            "-e",
            &format!("LISTEN_ADDR=0.0.0.0:8080"),
            RAUTHY_IMAGE,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    Ok(RauthyInstance {
        port,
        child: Some(child),
    })
}

pub async fn wait_until_healthy(port: u16) -> Result<(), BoxError> {
    let client = reqwest::Client::new();
    let start = std::time::Instant::now();

    loop {
        if start.elapsed() > HEALTH_TIMEOUT {
            return Err("rauthy health check timed out".into());
        }

        match client
            .get(format!("http://127.0.0.1:{}/auth/v1/health", port))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => return Ok(()),
            _ => tokio::time::sleep(HEALTH_POLL_INTERVAL).await,
        }
    }
}

pub async fn stop_rauthy(instance: &mut RauthyInstance) {
    if let Some(ref mut child) = instance.child {
        let _ = child.start_kill();
        let _ = child.wait().await;
    }
}
