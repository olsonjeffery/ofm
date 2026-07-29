use std::process::Stdio;
use thiserror::Error;
use tokio::process::Command;

const TIMEOUT_SECS: u64 = 120;

#[derive(Debug, Error)]
pub enum RamalamaError {
    #[error("ramalama not found in PATH")]
    NotFoundInPath,

    #[error("ramalama command failed: {stderr}")]
    CommandFailed { stderr: String },

    #[error("ramalama produced no output")]
    OutputEmpty,

    #[error("ramalama command timed out after {0}s")]
    TimedOut(u64),
}

pub async fn query(prompt: &str) -> Result<String, RamalamaError> {
    let child = match Command::new("ramalama")
        .args(["run", "ollama://phi4-mini", prompt])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            tracing::error!("ramalama not found in PATH — install ramalama or set OFM_RAMALAMA_PHI4_MINI_ENABLED=false");
            return Err(RamalamaError::NotFoundInPath);
        }
        Err(e) => {
            tracing::error!("failed to spawn ramalama: {e}");
            return Err(RamalamaError::CommandFailed {
                stderr: e.to_string(),
            });
        }
    };

    let output = match tokio::time::timeout(
        std::time::Duration::from_secs(TIMEOUT_SECS),
        child.wait_with_output(),
    )
    .await
    {
        Ok(Ok(o)) => o,
        Ok(Err(e)) => {
            tracing::error!("ramalama IO error: {e}");
            return Err(RamalamaError::CommandFailed {
                stderr: e.to_string(),
            });
        }
        Err(_) => return Err(RamalamaError::TimedOut(TIMEOUT_SECS)),
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        tracing::error!("ramalama exited with non-zero status: {stderr}");
        return Err(RamalamaError::CommandFailed { stderr });
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if stdout.is_empty() {
        return Err(RamalamaError::OutputEmpty);
    }

    Ok(stdout)
}

#[cfg(test)]
#[allow(clippy::await_holding_lock)]
mod tests {
    use super::*;
    use std::sync::LazyLock;

    static ENV_LOCK: LazyLock<std::sync::Mutex<()>> = LazyLock::new(|| std::sync::Mutex::new(()));

    fn write_shim(dir: &std::path::Path, script: &str) -> String {
        let shim_path = dir.join("ramalama");
        std::fs::write(&shim_path, script).unwrap();
        let mut perms = std::fs::metadata(&shim_path).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
        std::fs::set_permissions(&shim_path, perms).unwrap();
        shim_path.to_str().unwrap().to_string()
    }

    fn with_path(path: &str) -> String {
        let prev = std::env::var("PATH").unwrap_or_default();
        std::env::set_var("PATH", path);
        prev
    }

    #[tokio::test]
    async fn test_not_in_path() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let prev = with_path(dir.path().to_str().unwrap());
        let result = query("hi").await;
        std::env::set_var("PATH", &prev);
        assert!(matches!(result, Err(RamalamaError::NotFoundInPath)));
    }

    #[tokio::test]
    async fn test_empty_output() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let script = "#!/bin/sh\nexit 0";
        write_shim(dir.path(), script);
        let prev = with_path(dir.path().to_str().unwrap());
        let result = query("hi").await;
        std::env::set_var("PATH", &prev);
        assert!(matches!(result, Err(RamalamaError::OutputEmpty)));
    }

    #[tokio::test]
    async fn test_nonzero_exit() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let script = "#!/bin/sh\necho 'model not found' >&2\nexit 1";
        write_shim(dir.path(), script);
        let prev = with_path(dir.path().to_str().unwrap());
        let result = query("hi").await;
        std::env::set_var("PATH", &prev);
        match result {
            Err(RamalamaError::CommandFailed { stderr }) => {
                assert!(stderr.contains("model not found"), "stderr: {stderr}");
            }
            other => panic!("expected CommandFailed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_success() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let script = "#!/bin/sh\necho 'pineapple'";
        write_shim(dir.path(), script);
        let prev = with_path(dir.path().to_str().unwrap());
        let result = query("Reply with just the word pineapple").await;
        std::env::set_var("PATH", &prev);
        assert_eq!(result.unwrap(), "pineapple");
    }

    #[tokio::test]
    async fn test_real_ramalama_query() {
        // ENV_LOCK ensures PATH is not concurrently modified by other tests.
        let _guard = ENV_LOCK.lock().unwrap();
        if std::process::Command::new("which")
            .arg("ramalama")
            .output()
            .is_err()
        {
            eprintln!("skipping: ramalama not on PATH");
            return;
        }
        let result = query("Reply with just the word pineapple").await;
        match result {
            Ok(ref text) => assert!(
                text.to_lowercase().contains("pineapple"),
                "response: {text}"
            ),
            Err(RamalamaError::NotFoundInPath) => {
                eprintln!("skipping: ramalama not found in PATH at query time");
            }
            Err(e) => panic!("real ramalama query failed: {e}"),
        }
    }
}
