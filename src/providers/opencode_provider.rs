use std::path::{Path, PathBuf};
use std::sync::Mutex;

use async_trait::async_trait;
use base64::Engine;
use serde::Deserialize;
use tempfile::TempDir;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use uuid::Uuid;

use crate::providers::config::{merge_configs, ProviderConfig as PConfig, ProviderConfigDir};
use crate::providers::types::{ProviderEvent, ResumeInput, TurnInput};
use crate::providers::{HarnessConfig, LlmProvider, ProviderError};

pub struct OpenCodeProvider {
    config: HarnessConfig,
    provider_snippet: String,
    server: Mutex<Option<OpenCodeServer>>,
    working_dir: Mutex<Option<PathBuf>>,
    http_client: reqwest::Client,
}

struct OpenCodeServer {
    child: std::process::Child,
    port: u16,
    hostname: String,
    password: Option<String>,
    _temp_dir: TempDir,
}

impl OpenCodeProvider {
    pub async fn new(config: &HarnessConfig, config_root: &Path) -> Result<Self, ProviderError> {
        let cfg_dir = ProviderConfigDir::new(config_root);
        let provider_cfg = cfg_dir.load_provider_config(&config.provider_config_ref)?;
        Ok(Self {
            config: config.clone(),
            provider_snippet: provider_cfg.raw_snippet,
            server: Mutex::new(None),
            working_dir: Mutex::new(None),
            http_client: reqwest::Client::new(),
        })
    }

    fn server_details(&self) -> Option<(String, String)> {
        let guard = self.server.lock().unwrap();
        guard.as_ref().map(|s| {
            let base = format!("http://{}:{}", s.hostname, s.port);
            let pw = s.password.clone().unwrap_or_default();
            (base, pw)
        })
    }

    async fn do_transient<F, Fut, T>(
        config_ref: &str,
        snippet: &str,
        f: F,
    ) -> Result<T, ProviderError>
    where
        F: FnOnce(reqwest::Client, String, String) -> Fut + Send,
        Fut: std::future::Future<Output = Result<T, ProviderError>> + Send,
    {
        let (_server, client) = spawn_transient_server(config_ref, snippet).await?;
        let base_url = format!("http://{}:{}", _server.hostname, _server.port);
        let password = _server.password.unwrap_or_default();
        let result = f(client, base_url, password).await;
        let _ = std::process::Command::new("kill")
            .arg(_server.child.id().to_string())
            .status();
        result
    }
}

fn basic_auth_header(password: impl AsRef<str>) -> String {
    let encoded =
        base64::engine::general_purpose::STANDARD.encode(format!("opencode:{}", password.as_ref()));
    format!("Basic {encoded}")
}

fn pick_free_port() -> Result<u16, ProviderError> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(ProviderError::Io)?;
    let port = listener.local_addr().map_err(ProviderError::Io)?.port();
    drop(listener);
    Ok(port)
}

async fn wait_for_health(
    client: &reqwest::Client,
    base_url: &str,
    password: &str,
    mut child: Option<&mut std::process::Child>,
) -> Result<(), ProviderError> {
    let url = format!("{base_url}/global/health");
    for i in 0..20 {
        // Check if the child process has exited early
        if let Some(child) = child.as_mut() {
            if let Some(status) = child.try_wait().map_err(ProviderError::Io)? {
                return Err(ProviderError::Config(format!(
                    "opencode process exited prematurely with status: {status}"
                )));
            }
        }

        match client
            .get(&url)
            .header("Authorization", basic_auth_header(password))
            .timeout(std::time::Duration::from_millis(500))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => return Ok(()),
            _ => {
                if i == 19 {
                    return Err(ProviderError::Timeout);
                }
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
        }
    }
    Err(ProviderError::Timeout)
}

async fn spawn_opencode_server(
    config_ref: &str,
    snippet: &str,
) -> Result<OpenCodeServer, ProviderError> {
    let base_config = r#"{"provider":{},"telemetry":{"enabled":false}}"#;
    let provider_cfg = PConfig {
        harness: "opencode".to_string(),
        config_ref: config_ref.to_string(),
        raw_snippet: snippet.to_string(),
    };
    let merged = merge_configs(base_config, &provider_cfg)?;
    tracing::info!(
        config_ref = %config_ref,
        config = %merged,
        "Merged opencode provider configuration"
    );
    let temp_dir = TempDir::new().map_err(ProviderError::Io)?;
    std::fs::write(temp_dir.path().join("opencode.json"), &merged).map_err(ProviderError::Io)?;

    let port = pick_free_port()?;
    let hostname = "127.0.0.1".to_string();
    let password = Uuid::new_v4().to_string();

    let child = std::process::Command::new("opencode")
        .arg("serve")
        .arg("--port")
        .arg(port.to_string())
        .arg("--hostname")
        .arg(&hostname)
        .env("OPENCODE_CONFIG", temp_dir.path())
        .env("OPENCODE_SERVER_PASSWORD", &password)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ProviderError::Protocol("opencode binary not found in PATH".to_string())
            } else {
                ProviderError::Io(e)
            }
        })?;

    Ok(OpenCodeServer {
        child,
        port,
        hostname,
        password: Some(password),
        _temp_dir: temp_dir,
    })
}

async fn spawn_transient_server(
    config_ref: &str,
    snippet: &str,
) -> Result<(OpenCodeServer, reqwest::Client), ProviderError> {
    let mut server = spawn_opencode_server(config_ref, snippet).await?;
    let client = reqwest::Client::new();
    wait_for_health(
        &client,
        &format!("http://{}:{}", server.hostname, server.port),
        server.password.as_deref().unwrap_or(""),
        Some(&mut server.child),
    )
    .await?;
    Ok((server, client))
}

async fn fetch_models(
    client: &reqwest::Client,
    base_url: &str,
    password: &str,
) -> Result<Vec<String>, ProviderError> {
    let resp = client
        .get(format!("{base_url}/config/providers"))
        .header("Authorization", basic_auth_header(password))
        .send()
        .await?;
    let providers: Vec<serde_json::Value> = resp.json().await.unwrap_or_default();
    let models: Vec<String> = providers
        .iter()
        .filter_map(|p| {
            p.get("defaultModel")
                .or_else(|| p.get("model"))
                .and_then(|m| m.as_str())
                .map(|s| s.to_string())
        })
        .collect();
    if models.is_empty() {
        Ok(vec!["default".to_string()])
    } else {
        Ok(models)
    }
}

async fn one_shot_with_server(
    client: &reqwest::Client,
    base_url: &str,
    password: &str,
    prompt: &str,
    model: &str,
) -> Result<String, ProviderError> {
    let session_resp = client
        .post(format!("{base_url}/session"))
        .header("Authorization", basic_auth_header(password))
        .json(&serde_json::json!({"title": "one-shot"}))
        .send()
        .await?;
    if !session_resp.status().is_success() {
        return Err(ProviderError::Protocol(
            "failed to create one-shot session".into(),
        ));
    }
    let session_id: String = session_resp
        .json::<OpenCodeSession>()
        .await
        .map(|s| s.id)
        .unwrap_or_else(|_| Uuid::new_v4().to_string());

    let msg_resp = client
        .post(format!("{base_url}/session/{session_id}/prompt_async"))
        .header("Authorization", basic_auth_header(password))
        .json(&serde_json::json!({
            "model": model,
            "parts": [{"type": "text", "text": prompt}]
        }))
        .send()
        .await?;
    if !msg_resp.status().is_success() {
        return Err(ProviderError::Protocol(
            "failed to send one-shot message".into(),
        ));
    }

    let result = collect_response_via_sse(client, base_url, password, &session_id).await;

    let _ = client
        .delete(format!("{base_url}/session/{session_id}"))
        .header("Authorization", basic_auth_header(password))
        .send()
        .await;

    result
}

fn map_opencode_event_to_provider_event(line: &str) -> Option<ProviderEvent> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    match v.get("type").and_then(|t| t.as_str()) {
        Some("message.updated") => {
            let role = v.get("role").and_then(|r| r.as_str()).unwrap_or("");
            if role == "assistant" {
                let content = v.get("content").and_then(|c| c.as_str()).unwrap_or("");
                if !content.is_empty() {
                    Some(ProviderEvent::TextChunk {
                        delta: content.to_string(),
                    })
                } else {
                    None
                }
            } else {
                None
            }
        }
        Some("tool_use") => {
            let tool_name = v
                .get("tool_name")
                .and_then(|t| t.as_str())
                .unwrap_or("unknown")
                .to_string();
            let tool_use_id = v
                .get("tool_use_id")
                .and_then(|t| t.as_str())
                .map(|s| s.to_string());
            let input = v.get("input").cloned().unwrap_or(serde_json::Value::Null);
            Some(ProviderEvent::ToolUse {
                tool_name,
                tool_use_id,
                input,
            })
        }
        Some("tool_result") => {
            let tool_use_id = v
                .get("tool_use_id")
                .and_then(|t| t.as_str())
                .map(|s| s.to_string());
            let result = v
                .get("result")
                .and_then(|r| r.as_str())
                .unwrap_or("")
                .to_string();
            Some(ProviderEvent::ToolResult {
                tool_use_id,
                result,
            })
        }
        Some("thinking") => {
            let thinking = v
                .get("thinking")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string();
            Some(ProviderEvent::Thinking { thinking })
        }
        Some("error") => {
            let error = v
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("unknown error")
                .to_string();
            Some(ProviderEvent::Error { error })
        }
        Some("done") | Some("completed") => Some(ProviderEvent::Done(serde_json::Value::Null)),
        _ => None,
    }
}

fn drain_sse_lines(buf: &mut Vec<u8>) -> Vec<String> {
    let mut events = Vec::new();
    while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
        let line_bytes: Vec<u8> = buf.drain(..=pos).collect();
        let line = String::from_utf8_lossy(&line_bytes[..line_bytes.len().saturating_sub(1)]);
        let trimmed = line.trim();
        if !trimmed.is_empty() && !trimmed.starts_with(':') {
            if let Some(data) = trimmed.strip_prefix("data: ") {
                events.push(data.to_string());
            }
        }
    }
    events
}

async fn read_sse_to_completion(
    client: &reqwest::Client,
    url: &str,
    password: &str,
    tx: mpsc::Sender<ProviderEvent>,
) {
    let Ok(resp) = client
        .get(url)
        .header("Authorization", basic_auth_header(password))
        .send()
        .await
    else {
        return;
    };

    let mut buf = Vec::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk_result) = stream.next().await {
        let Ok(chunk) = chunk_result else {
            tracing::warn!("SSE chunk error: {:?}", chunk_result.err());
            return;
        };
        buf.extend_from_slice(&chunk);
        for data in drain_sse_lines(&mut buf) {
            if let Some(event) = map_opencode_event_to_provider_event(&data) {
                let is_done = matches!(&event, ProviderEvent::Done(_));
                if tx.blocking_send(event).is_err() {
                    return;
                }
                if is_done {
                    return;
                }
            }
        }
    }
}

async fn collect_response_via_sse(
    client: &reqwest::Client,
    base_url: &str,
    password: &str,
    _session_id: &str,
) -> Result<String, ProviderError> {
    let events_url = format!("{base_url}/event");
    let resp = client
        .get(&events_url)
        .header("Authorization", basic_auth_header(password))
        .send()
        .await?;

    let mut response = String::new();
    let mut buf = Vec::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk_result) = stream.next().await {
        let Ok(chunk) = chunk_result else {
            tracing::warn!("SSE chunk error: {:?}", chunk_result.err());
            break;
        };
        buf.extend_from_slice(&chunk);
        for data in drain_sse_lines(&mut buf) {
            if let Some(event) = map_opencode_event_to_provider_event(&data) {
                match event {
                    ProviderEvent::TextChunk { delta } => response.push_str(&delta),
                    ProviderEvent::Text { text } => response.push_str(&text),
                    ProviderEvent::Error { error } => {
                        tracing::warn!("one_shot error: {error}");
                        return Err(ProviderError::Protocol(error));
                    }
                    ProviderEvent::Done(_) => {
                        if response.is_empty() {
                            return Err(ProviderError::Protocol(
                                "no response from opencode".into(),
                            ));
                        }
                        return Ok(response);
                    }
                    _ => {}
                }
            }
        }
    }
    if response.is_empty() {
        Err(ProviderError::Protocol("no response from opencode".into()))
    } else {
        Ok(response)
    }
}

#[derive(Debug, Deserialize)]
struct OpenCodeSession {
    id: String,
}

#[async_trait]
impl LlmProvider for OpenCodeProvider {
    async fn get_models_list(&self) -> Result<Vec<String>, ProviderError> {
        if let Some((base_url, password)) = self.server_details() {
            fetch_models(&self.http_client, &base_url, &password).await
        } else {
            let config_ref = self.config.provider_config_ref.clone();
            let snippet = self.provider_snippet.clone();
            Self::do_transient(
                &config_ref,
                &snippet,
                move |client, base_url, password| async move {
                    fetch_models(&client, &base_url, &password).await
                },
            )
            .await
        }
    }

    async fn start(&mut self, working_dir: &Path) -> Result<(), ProviderError> {
        let mut server =
            spawn_opencode_server(&self.config.provider_config_ref, &self.provider_snippet).await?;
        wait_for_health(
            &self.http_client,
            &format!("http://{}:{}", server.hostname, server.port),
            server.password.as_deref().unwrap_or(""),
            Some(&mut server.child),
        )
        .await?;
        *self.server.lock().unwrap() = Some(server);
        *self.working_dir.lock().unwrap() = Some(working_dir.to_path_buf());
        Ok(())
    }

    async fn start_turn(
        &self,
        input: TurnInput,
    ) -> Result<mpsc::Receiver<ProviderEvent>, ProviderError> {
        let (base_url, password) = self.server_details().ok_or(ProviderError::NotStarted)?;

        let session_resp = self
            .http_client
            .post(format!("{base_url}/session"))
            .header("Authorization", basic_auth_header(&password))
            .json(&serde_json::json!({"title": "ofm session"}))
            .send()
            .await?;
        if !session_resp.status().is_success() {
            return Err(ProviderError::Protocol(format!(
                "failed to create session: {}",
                session_resp.status()
            )));
        }
        let session_id: String = session_resp
            .json::<OpenCodeSession>()
            .await
            .map(|s| s.id)
            .unwrap_or_else(|_| Uuid::new_v4().to_string());

        let msg_resp = self
            .http_client
            .post(format!("{base_url}/session/{session_id}/prompt_async"))
            .header("Authorization", basic_auth_header(&password))
            .json(&serde_json::json!({
                "model": input.model,
                "parts": [{"type": "text", "text": input.prompt}]
            }))
            .send()
            .await?;
        if !msg_resp.status().is_success() {
            return Err(ProviderError::Protocol(format!(
                "failed to send message: {}",
                msg_resp.status()
            )));
        }

        let (tx, rx) = mpsc::channel(256);
        let client = self.http_client.clone();
        let events_url = format!("{base_url}/event");
        let pw = password.clone();

        tokio::spawn(async move {
            read_sse_to_completion(&client, &events_url, &pw, tx).await;
        });

        Ok(rx)
    }

    async fn resume_turn(
        &self,
        _input: ResumeInput,
    ) -> Result<mpsc::Receiver<ProviderEvent>, ProviderError> {
        Err(ProviderError::Protocol(
            "resume_turn not supported by OpenCodeProvider".into(),
        ))
    }

    async fn abort_turn(&self) -> Result<(), ProviderError> {
        if let Some((base_url, password)) = self.server_details() {
            let _ = self
                .http_client
                .post(format!("{base_url}/session/current/abort"))
                .header("Authorization", basic_auth_header(&password))
                .send()
                .await;
        }
        Ok(())
    }

    async fn one_shot_prompt(&self, prompt: &str, model: &str) -> Result<String, ProviderError> {
        if let Some((base_url, password)) = self.server_details() {
            one_shot_with_server(&self.http_client, &base_url, &password, prompt, model).await
        } else {
            let config_ref = self.config.provider_config_ref.clone();
            let snippet = self.provider_snippet.clone();
            let prompt = prompt.to_string();
            let model = model.to_string();
            Self::do_transient(
                &config_ref,
                &snippet,
                move |client, base_url, password| async move {
                    one_shot_with_server(&client, &base_url, &password, &prompt, &model).await
                },
            )
            .await
        }
    }

    async fn shutdown(&mut self) -> Result<bool, ProviderError> {
        let mut guard = self.server.lock().unwrap();
        if let Some(s) = guard.as_mut() {
            let _ = s.child.stdin.take();
            let _ = s.child.kill();
            let _ = s.child.wait();
            *guard = None;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_opencode_event_message_updated_assistant() {
        let line = r#"{"type":"message.updated","role":"assistant","content":"Hello"}"#;
        let event = map_opencode_event_to_provider_event(line);
        assert!(matches!(event, Some(ProviderEvent::TextChunk { delta }) if delta == "Hello"));
    }

    #[test]
    fn test_map_opencode_event_tool_use() {
        let line =
            r#"{"type":"tool_use","tool_name":"read","tool_use_id":"id1","input":{"path":"/tmp"}}"#;
        let event = map_opencode_event_to_provider_event(line);
        assert!(
            matches!(event, Some(ProviderEvent::ToolUse { tool_name, tool_use_id: Some(id), .. }) if tool_name == "read" && id == "id1")
        );
    }

    #[test]
    fn test_map_opencode_event_tool_result() {
        let line = r#"{"type":"tool_result","tool_use_id":"id1","result":"ok"}"#;
        let event = map_opencode_event_to_provider_event(line);
        assert!(
            matches!(event, Some(ProviderEvent::ToolResult { tool_use_id: Some(id), result }) if id == "id1" && result == "ok")
        );
    }

    #[test]
    fn test_map_opencode_event_thinking() {
        let line = r#"{"type":"thinking","thinking":"hmm"}"#;
        let event = map_opencode_event_to_provider_event(line);
        assert!(matches!(event, Some(ProviderEvent::Thinking { thinking }) if thinking == "hmm"));
    }

    #[test]
    fn test_map_opencode_event_error() {
        let line = r#"{"type":"error","error":"something went wrong"}"#;
        let event = map_opencode_event_to_provider_event(line);
        assert!(
            matches!(event, Some(ProviderEvent::Error { error }) if error == "something went wrong")
        );
    }

    #[test]
    fn test_map_opencode_event_done() {
        let line = r#"{"type":"done"}"#;
        let event = map_opencode_event_to_provider_event(line);
        assert!(matches!(event, Some(ProviderEvent::Done(_))));
    }

    #[test]
    fn test_map_opencode_event_completed() {
        let line = r#"{"type":"completed"}"#;
        let event = map_opencode_event_to_provider_event(line);
        assert!(matches!(event, Some(ProviderEvent::Done(_))));
    }

    #[test]
    fn test_map_opencode_event_unknown_type() {
        let line = r#"{"type":"unknown","data":"foo"}"#;
        let event = map_opencode_event_to_provider_event(line);
        assert!(event.is_none());
    }

    #[test]
    fn test_map_opencode_event_user_message_ignored() {
        let line = r#"{"type":"message.updated","role":"user","content":"hello"}"#;
        let event = map_opencode_event_to_provider_event(line);
        assert!(event.is_none());
    }
}
