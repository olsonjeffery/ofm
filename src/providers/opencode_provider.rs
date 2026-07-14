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
use crate::providers::types::{ProviderEvent, QuestionOption, ResumeInput, TurnInput};
use crate::providers::{HarnessConfig, LlmProvider, ProviderError};

pub struct OpenCodeProvider {
    config: HarnessConfig,
    provider_snippet: String,
    provider_id: String,
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
        let provider_id = Self::extract_provider_id(&provider_cfg.raw_snippet)
            .unwrap_or_else(|| "default".to_string());
        Ok(Self {
            config: config.clone(),
            provider_snippet: provider_cfg.raw_snippet,
            provider_id,
            server: Mutex::new(None),
            working_dir: Mutex::new(None),
            http_client: reqwest::Client::new(),
        })
    }

    fn extract_provider_id(snippet: &str) -> Option<String> {
        let v: serde_json::Value = serde_json::from_str(snippet).ok()?;
        v.get("provider")?.as_object()?.keys().next().cloned()
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
    working_dir: Option<&std::path::Path>,
) -> Result<OpenCodeServer, ProviderError> {
    let base_config = r#"{"provider":{},"permission":{"edit":"allow","bash":"allow","webfetch":"allow","doom_loop":"allow","external_directory":"allow"}}"#;
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
    let config_path = temp_dir.path().join("opencode.json");
    std::fs::write(&config_path, &merged).map_err(ProviderError::Io)?;

    let port = pick_free_port()?;
    let hostname = "127.0.0.1".to_string();
    let password = Uuid::new_v4().to_string();

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
    if let Some(dir) = working_dir {
        cmd.current_dir(dir);
    }
    let child = cmd.spawn().map_err(|e| {
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
    let mut server = spawn_opencode_server(config_ref, snippet, None).await?;
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
    let resp_status = resp.status();
    if !resp_status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        tracing::warn!("GET /config/providers failed: {resp_status} — body: {body}");
        return Ok(vec!["default".to_string()]);
    }
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
    provider_id: &str,
) -> Result<String, ProviderError> {
    let session_resp = client
        .post(format!("{base_url}/session"))
        .header("Authorization", basic_auth_header(password))
        .json(&serde_json::json!({"title": "one-shot"}))
        .send()
        .await?;
    let session_status = session_resp.status();
    if !session_status.is_success() {
        let body = session_resp.text().await.unwrap_or_default();
        return Err(ProviderError::Protocol(format!(
            "failed to create one-shot session: {session_status} — body: {body}"
        )));
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
            "model": {"providerID": provider_id, "modelID": model},
            "parts": [{"type": "text", "text": prompt}]
        }))
        .send()
        .await?;
    let msg_status = msg_resp.status();
    if !msg_status.is_success() {
        let body = msg_resp.text().await.unwrap_or_default();
        return Err(ProviderError::Protocol(format!(
            "failed to send one-shot message: {msg_status} — body: {body}"
        )));
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

    // opencode wraps events in a GlobalEvent: { directory, payload: { type, properties } }
    let payload = v.get("payload").unwrap_or(&v);

    let event_type = match payload.get("type").and_then(|t| t.as_str()) {
        Some(t) => t,
        None => {
            tracing::debug!("opencode event missing type field: {v:?}");
            return None;
        }
    };

    match event_type {
        "message.part.updated" => {
            let part = payload.get("properties").and_then(|p| p.get("part"));
            let part_type = part.and_then(|p| p.get("type")).and_then(|t| t.as_str());
            let delta = payload
                .get("properties")
                .and_then(|p| p.get("delta"))
                .and_then(|d| d.as_str())
                .unwrap_or("");
            match part_type {
                Some("text") => {
                    let text = part
                        .and_then(|p| p.get("text"))
                        .and_then(|t| t.as_str())
                        .unwrap_or(delta);
                    if !text.is_empty() {
                        Some(ProviderEvent::TextChunk {
                            delta: text.to_string(),
                        })
                    } else {
                        None
                    }
                }
                Some("reasoning") => {
                    let thinking = part
                        .and_then(|p| p.get("text"))
                        .and_then(|t| t.as_str())
                        .unwrap_or("");
                    Some(ProviderEvent::Thinking {
                        thinking: thinking.to_string(),
                    })
                }
                Some("tool") => {
                    let call_id = part
                        .and_then(|p| p.get("callID"))
                        .and_then(|c| c.as_str())
                        .map(|s| s.to_string());
                    let state = part.and_then(|p| p.get("state"));
                    let status = state.and_then(|s| s.get("status")).and_then(|s| s.as_str());
                    if status == Some("completed") || status == Some("error") {
                        let result = state
                            .and_then(|s| s.get("output"))
                            .or_else(|| state.and_then(|s| s.get("error")))
                            .and_then(|r| r.as_str())
                            .unwrap_or("")
                            .to_string();
                        Some(ProviderEvent::ToolResult {
                            tool_use_id: call_id,
                            result,
                        })
                    } else {
                        let tool_name = part
                            .and_then(|p| p.get("tool"))
                            .and_then(|t| t.as_str())
                            .unwrap_or("unknown")
                            .to_string();
                        let input = state
                            .and_then(|s| s.get("input"))
                            .cloned()
                            .unwrap_or(serde_json::Value::Null);
                        Some(ProviderEvent::ToolUse {
                            tool_name,
                            tool_use_id: call_id,
                            input,
                        })
                    }
                }
                _ => {
                    tracing::debug!("opencode unhandled part type: {part_type:?} — {v:?}");
                    None
                }
            }
        }
        "message.updated" => {
            let info = payload.get("properties").and_then(|p| p.get("info"));
            let role = info.and_then(|i| i.get("role")).and_then(|r| r.as_str());
            if role == Some("assistant") {
                let parts = info
                    .and_then(|i| i.as_object())
                    .and_then(|o| {
                        o.get("parts").or_else(|| {
                            // fallback: check parts in properties directly
                            payload.get("properties").and_then(|p| p.get("parts"))
                        })
                    })
                    .and_then(|p| p.as_array());
                if let Some(parts) = parts {
                    for part in parts {
                        if part.get("type").and_then(|t| t.as_str()) == Some("text") {
                            if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                                if !text.is_empty() {
                                    return Some(ProviderEvent::TextChunk {
                                        delta: text.to_string(),
                                    });
                                }
                            }
                        }
                    }
                }
            }
            None
        }
        "error" => {
            let props = payload.get("properties");
            let error = props
                .and_then(|p| p.get("error"))
                .and_then(|e| e.as_str())
                .or_else(|| {
                    props
                        .and_then(|p| p.get("info"))
                        .and_then(|i| i.get("error"))
                        .and_then(|e| e.as_str())
                })
                .unwrap_or("unknown error")
                .to_string();
            Some(ProviderEvent::Error { error })
        }
        "done" | "completed" => Some(ProviderEvent::Done(serde_json::Value::Null)),
        "server.connected" | "session.status" | "session.idle" | "session.created"
        | "session.updated" | "permission.updated" => None,
        "question.asked" => {
            let props = payload.get("properties")?;
            let qid = props.get("sessionID")?.as_str()?;
            let question = props
                .get("questions")?
                .as_array()?
                .first()
                .and_then(|q| q.get("question"))
                .and_then(|q| q.as_str())
                .unwrap_or("");
            let header = props
                .get("questions")?
                .as_array()?
                .first()
                .and_then(|q| q.get("header"))
                .and_then(|h| h.as_str());
            let options = props
                .get("questions")?
                .as_array()?
                .first()
                .and_then(|q| q.get("options"))
                .and_then(|o| serde_json::from_value::<Vec<QuestionOption>>(o.clone()).ok())
                .unwrap_or_default();
            Some(ProviderEvent::QuestionAsked {
                session_id: qid.to_string(),
                question: question.to_string(),
                header: header.map(String::from),
                options,
            })
        }
        _ => {
            tracing::debug!("opencode unhandled event type: {event_type} — {v:?}");
            None
        }
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
    session_id: &str,
    tx: mpsc::Sender<ProviderEvent>,
) {
    let resp = match client
        .get(url)
        .header("Authorization", basic_auth_header(password))
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => r,
        Ok(r) => {
            tracing::warn!("SSE connection returned {} for {url}", r.status());
            let _ = tx.send(ProviderEvent::Done(serde_json::Value::Null)).await;
            return;
        }
        Err(e) => {
            tracing::warn!("SSE connection failed to {url}: {e}");
            let _ = tx.send(ProviderEvent::Done(serde_json::Value::Null)).await;
            return;
        }
    };

    let mut buf = Vec::new();
    let mut stream = resp.bytes_stream();
    let mut line_count = 0usize;
    while let Some(chunk_result) = stream.next().await {
        let chunk = match chunk_result {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("SSE chunk error: {e}");
                let _ = tx.send(ProviderEvent::Done(serde_json::Value::Null)).await;
                return;
            }
        };
        buf.extend_from_slice(&chunk);
        for data in drain_sse_lines(&mut buf) {
            line_count += 1;
            // Log SSE data to console, skipping noisy text deltas and part deltas
            let skip = serde_json::from_str::<serde_json::Value>(&data)
                .ok()
                .is_some_and(|v| {
                    let t = v
                        .get("payload")
                        .and_then(|p| p.get("type"))
                        .and_then(|t| t.as_str());
                    // Skip streaming text deltas and message.part.delta events
                    t == Some("message.part.delta")
                        || (t == Some("message.part.updated")
                            && v.get("payload")
                                .and_then(|p| p.get("properties"))
                                .and_then(|p| p.get("part"))
                                .and_then(|p| p.get("type"))
                                .and_then(|t| t.as_str())
                                == Some("text"))
                });
            if !skip {
                tracing::info!("SSE #{line_count}: {data}");
            }
            // Check for session lifecycle events before dispatching
            let v: serde_json::Value = match serde_json::from_str(&data) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let event_type = v
                .get("payload")
                .and_then(|p| p.get("type"))
                .and_then(|t| t.as_str());
            match event_type {
                Some("session.idle") | Some("session.status") => {
                    let sid = v
                        .get("payload")
                        .and_then(|p| p.get("properties"))
                        .and_then(|p| p.get("sessionID"))
                        .and_then(|s| s.as_str());
                    if sid == Some(session_id) {
                        if event_type == Some("session.status") {
                            let status_type = v
                                .get("payload")
                                .and_then(|p| p.get("properties"))
                                .and_then(|p| p.get("status"))
                                .and_then(|s| s.get("type"))
                                .and_then(|t| t.as_str());
                            if status_type != Some("idle") {
                                continue;
                            }
                        }
                        tracing::debug!("session {session_id} idle — done");
                        let _ = tx.send(ProviderEvent::Done(serde_json::Value::Null)).await;
                        return;
                    }
                }
                Some("session.error") => {
                    let sid = v
                        .get("payload")
                        .and_then(|p| p.get("properties"))
                        .and_then(|p| p.get("sessionID"))
                        .and_then(|s| s.as_str());
                    if sid == Some(session_id) {
                        let msg = v
                            .get("payload")
                            .and_then(|p| p.get("properties"))
                            .and_then(|p| p.get("error"))
                            .and_then(|e| e.as_str())
                            .unwrap_or("session error");
                        tracing::warn!("session {session_id} error: {msg}");
                        let _ = tx
                            .send(ProviderEvent::Error {
                                error: msg.to_string(),
                            })
                            .await;
                        return;
                    }
                }
                Some("message.updated") => {
                    // Assistant message with finish set means message is complete
                    let info = v
                        .get("payload")
                        .and_then(|p| p.get("properties"))
                        .and_then(|p| p.get("info"));
                    let sid = info
                        .and_then(|i| i.get("sessionID"))
                        .and_then(|s| s.as_str());
                    let role = info.and_then(|i| i.get("role")).and_then(|r| r.as_str());
                    let finish = info.and_then(|i| i.get("finish")).and_then(|f| f.as_str());
                    if sid == Some(session_id) && role == Some("assistant") && finish.is_some() {
                        tracing::debug!(
                            "session {session_id} message finished ({finish:?}) — done"
                        );
                        let _ = tx.send(ProviderEvent::Done(serde_json::Value::Null)).await;
                        return;
                    }
                }
                _ => {}
            }
            let raw_type = v
                .get("payload")
                .and_then(|p| p.get("type"))
                .and_then(|t| t.as_str());
            if raw_type == Some("question.asked") {
                let qsid = v
                    .get("payload")
                    .and_then(|p| p.get("properties"))
                    .and_then(|p| p.get("sessionID"))
                    .and_then(|s| s.as_str());
                if qsid == Some(session_id) {
                    if let Some(event) = map_opencode_event_to_provider_event(&data) {
                        let _ = tx.send(event).await;
                    }
                    return;
                }
                continue;
            }
            if let Some(event) = map_opencode_event_to_provider_event(&data) {
                let is_done = matches!(&event, ProviderEvent::Done(_));
                let is_question = matches!(&event, ProviderEvent::QuestionAsked { .. });
                if tx.send(event).await.is_err() {
                    return;
                }
                if is_done || is_question {
                    return;
                }
            }
        }
    }
    // Stream ended without explicit session.idle — still signal done
    tracing::debug!("SSE stream ended for session {session_id}");
    let _ = tx.send(ProviderEvent::Done(serde_json::Value::Null)).await;
}

async fn collect_response_via_sse(
    client: &reqwest::Client,
    base_url: &str,
    password: &str,
    session_id: &str,
) -> Result<String, ProviderError> {
    let events_url = format!("{base_url}/event");
    let resp = client
        .get(&events_url)
        .header("Authorization", basic_auth_header(password))
        .send()
        .await?;
    let resp_status = resp.status();
    if !resp_status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(ProviderError::Protocol(format!(
            "failed to connect to event stream: {resp_status} — body: {body}"
        )));
    }

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
            // Log SSE data, skipping noisy text deltas and part deltas
            let skip = serde_json::from_str::<serde_json::Value>(&data)
                .ok()
                .is_some_and(|v| {
                    let t = v
                        .get("payload")
                        .and_then(|p| p.get("type"))
                        .and_then(|t| t.as_str());
                    t == Some("message.part.delta")
                        || (t == Some("message.part.updated")
                            && v.get("payload")
                                .and_then(|p| p.get("properties"))
                                .and_then(|p| p.get("part"))
                                .and_then(|p| p.get("type"))
                                .and_then(|t| t.as_str())
                                == Some("text"))
                });
            if !skip {
                tracing::info!("SSE data: {data}");
            }
            // Check for session lifecycle events
            let v: serde_json::Value = match serde_json::from_str(&data) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let event_type = v
                .get("payload")
                .and_then(|p| p.get("type"))
                .and_then(|t| t.as_str());
            match event_type {
                Some("session.idle") | Some("session.status") => {
                    let sid = v
                        .get("payload")
                        .and_then(|p| p.get("properties"))
                        .and_then(|p| p.get("sessionID"))
                        .and_then(|s| s.as_str());
                    if sid == Some(session_id) {
                        if event_type == Some("session.status") {
                            let status_type = v
                                .get("payload")
                                .and_then(|p| p.get("properties"))
                                .and_then(|p| p.get("status"))
                                .and_then(|s| s.get("type"))
                                .and_then(|t| t.as_str());
                            if status_type != Some("idle") {
                                continue;
                            }
                        }
                        if response.is_empty() {
                            return Err(ProviderError::Protocol(
                                "no response from opencode".into(),
                            ));
                        }
                        return Ok(response);
                    }
                }
                Some("session.error") => {
                    let sid = v
                        .get("payload")
                        .and_then(|p| p.get("properties"))
                        .and_then(|p| p.get("sessionID"))
                        .and_then(|s| s.as_str());
                    if sid == Some(session_id) {
                        let msg = v
                            .get("payload")
                            .and_then(|p| p.get("properties"))
                            .and_then(|p| p.get("error"))
                            .and_then(|e| e.as_str())
                            .unwrap_or("session error");
                        return Err(ProviderError::Protocol(msg.to_string()));
                    }
                }
                Some("message.updated") => {
                    let info = v
                        .get("payload")
                        .and_then(|p| p.get("properties"))
                        .and_then(|p| p.get("info"));
                    let sid = info
                        .and_then(|i| i.get("sessionID"))
                        .and_then(|s| s.as_str());
                    let role = info.and_then(|i| i.get("role")).and_then(|r| r.as_str());
                    let finish = info.and_then(|i| i.get("finish")).and_then(|f| f.as_str());
                    if sid == Some(session_id) && role == Some("assistant") && finish.is_some() {
                        if response.is_empty() {
                            return Err(ProviderError::Protocol(
                                "no response from opencode".into(),
                            ));
                        }
                        return Ok(response);
                    }
                }
                _ => {}
            }
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
        let mut server = spawn_opencode_server(
            &self.config.provider_config_ref,
            &self.provider_snippet,
            Some(working_dir),
        )
        .await?;
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

        let working_dir = self.working_dir.lock().unwrap().clone().unwrap_or_default();

        let session_resp = self
            .http_client
            .post(
                reqwest::Url::parse_with_params(
                    &format!("{base_url}/session"),
                    &[("directory", working_dir.to_string_lossy().as_ref())],
                )
                .map_err(|e| ProviderError::Protocol(e.to_string()))?,
            )
            .header("Authorization", basic_auth_header(&password))
            .json(&serde_json::json!({"title": "ofm session"}))
            .send()
            .await?;
        let session_status = session_resp.status();
        if !session_status.is_success() {
            let body = session_resp.text().await.unwrap_or_default();
            return Err(ProviderError::Protocol(format!(
                "failed to create session: {session_status} — body: {body}"
            )));
        }
        let session_id: String = session_resp
            .json::<OpenCodeSession>()
            .await
            .map(|s| s.id)
            .unwrap_or_else(|_| Uuid::new_v4().to_string());

        let (tx, rx) = mpsc::channel(256);
        let _ = tx
            .send(ProviderEvent::SessionStart {
                session_id: session_id.clone(),
            })
            .await;

        let msg_resp = self
            .http_client
            .post(format!("{base_url}/session/{session_id}/prompt_async"))
            .header("Authorization", basic_auth_header(&password))
            .json(&serde_json::json!({
                "model": {"providerID": self.provider_id, "modelID": input.model},
                "parts": [{"type": "text", "text": input.prompt}]
            }))
            .send()
            .await?;
        let msg_status = msg_resp.status();
        if !msg_status.is_success() {
            let body = msg_resp.text().await.unwrap_or_default();
            return Err(ProviderError::Protocol(format!(
                "failed to send message: {msg_status} — body: {body}"
            )));
        }

        let client = self.http_client.clone();
        let events_url = format!("{base_url}/event");
        let pw = password.clone();
        let sid = session_id.clone();

        tokio::spawn(async move {
            read_sse_to_completion(&client, &events_url, &pw, &sid, tx).await;
        });

        Ok(rx)
    }

    async fn resume_turn(
        &self,
        input: ResumeInput,
    ) -> Result<mpsc::Receiver<ProviderEvent>, ProviderError> {
        let (base_url, password) = self.server_details().ok_or(ProviderError::NotStarted)?;
        let session_id = input.session_id.clone();

        let last_text = input
            .messages
            .as_array()
            .and_then(|arr| arr.last())
            .and_then(|m| m.get("text"))
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();

        if last_text.is_empty() {
            return Err(ProviderError::Protocol("no user text to send".into()));
        }

        let msg_resp = self
            .http_client
            .post(format!("{base_url}/session/{session_id}/message"))
            .header("Authorization", basic_auth_header(&password))
            .json(&serde_json::json!({
                "parts": [{"type": "text", "text": last_text}]
            }))
            .send()
            .await?;
        if !msg_resp.status().is_success() {
            return Err(ProviderError::Protocol(format!(
                "failed to send resume message: {}",
                msg_resp.status()
            )));
        }

        let (tx, rx) = mpsc::channel(256);
        let client = self.http_client.clone();
        let events_url = format!("{base_url}/event");
        let pw = password.clone();
        let sid = session_id.clone();

        tokio::spawn(async move {
            read_sse_to_completion(&client, &events_url, &pw, &sid, tx).await;
        });

        Ok(rx)
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
        let provider_id = self.provider_id.clone();
        if let Some((base_url, password)) = self.server_details() {
            one_shot_with_server(
                &self.http_client,
                &base_url,
                &password,
                prompt,
                model,
                &provider_id,
            )
            .await
        } else {
            let config_ref = self.config.provider_config_ref.clone();
            let snippet = self.provider_snippet.clone();
            let prompt = prompt.to_string();
            let model = model.to_string();
            Self::do_transient(
                &config_ref,
                &snippet,
                move |client, base_url, password| async move {
                    one_shot_with_server(
                        &client,
                        &base_url,
                        &password,
                        &prompt,
                        &model,
                        &provider_id,
                    )
                    .await
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

    fn global_event(payload: &str) -> String {
        format!(r#"{{"directory":"/tmp","payload":{payload}}}"#)
    }

    #[test]
    fn test_map_opencode_event_text_chunk() {
        let line = global_event(
            r#"{"type":"message.part.updated","properties":{"part":{"type":"text","text":"Hello"},"delta":"Hello"}}"#,
        );
        let event = map_opencode_event_to_provider_event(&line);
        assert!(matches!(event, Some(ProviderEvent::TextChunk { delta }) if delta == "Hello"));
    }

    #[test]
    fn test_map_opencode_event_tool_use() {
        let line = global_event(
            r#"{"type":"message.part.updated","properties":{"part":{"type":"tool","tool":"read","callID":"id1","state":{"input":{"path":"/tmp"}}}}}"#,
        );
        let event = map_opencode_event_to_provider_event(&line);
        assert!(
            matches!(event, Some(ProviderEvent::ToolUse { tool_name, tool_use_id: Some(id), .. }) if tool_name == "read" && id == "id1")
        );
    }

    #[test]
    fn test_map_opencode_event_tool_result() {
        let line = global_event(
            r#"{"type":"message.part.updated","properties":{"part":{"type":"tool","callID":"id1","state":{"status":"completed","output":"ok"}}}}"#,
        );
        let event = map_opencode_event_to_provider_event(&line);
        assert!(
            matches!(event, Some(ProviderEvent::ToolResult { tool_use_id: Some(id), result }) if id == "id1" && result == "ok")
        );
    }

    #[test]
    fn test_map_opencode_event_thinking() {
        let line = global_event(
            r#"{"type":"message.part.updated","properties":{"part":{"type":"reasoning","text":"hmm"}}}"#,
        );
        let event = map_opencode_event_to_provider_event(&line);
        assert!(matches!(event, Some(ProviderEvent::Thinking { thinking }) if thinking == "hmm"));
    }

    #[test]
    fn test_map_opencode_event_error() {
        let line =
            global_event(r#"{"type":"error","properties":{"error":"something went wrong"}}"#);
        let event = map_opencode_event_to_provider_event(&line);
        assert!(
            matches!(event, Some(ProviderEvent::Error { error }) if error == "something went wrong")
        );
    }

    #[test]
    fn test_map_opencode_event_done() {
        let line = global_event(r#"{"type":"done","properties":{}}"#);
        let event = map_opencode_event_to_provider_event(&line);
        assert!(matches!(event, Some(ProviderEvent::Done(_))));
    }

    #[test]
    fn test_map_opencode_event_completed() {
        let line = global_event(r#"{"type":"completed","properties":{}}"#);
        let event = map_opencode_event_to_provider_event(&line);
        assert!(matches!(event, Some(ProviderEvent::Done(_))));
    }

    #[test]
    fn test_map_opencode_event_unknown_type() {
        let line = global_event(r#"{"type":"unknown","properties":{"data":"foo"}}"#);
        let event = map_opencode_event_to_provider_event(&line);
        assert!(event.is_none());
    }

    #[test]
    fn test_map_opencode_event_user_message_ignored() {
        let line = global_event(
            r#"{"type":"message.updated","properties":{"info":{"role":"user","parts":[{"type":"text","text":"hello"}]}}}"#,
        );
        let event = map_opencode_event_to_provider_event(&line);
        assert!(event.is_none());
    }

    #[test]
    fn test_map_opencode_event_question_asked() {
        let line = global_event(
            r#"{"type":"question.asked","properties":{"sessionID":"sess-1","questions":[{"question":"What model?","header":"Choose","options":[{"label":"gpt-4","description":"Fast"},{"label":"claude-3","description":"Smart"}]}]}}"#,
        );
        let event = map_opencode_event_to_provider_event(&line);
        assert!(matches!(
            event,
            Some(ProviderEvent::QuestionAsked {
                session_id,
                question,
                header,
                options,
            }) if session_id == "sess-1"
                && question == "What model?"
                && header == Some("Choose".to_string())
                && options.len() == 2
                && options[0].label == "gpt-4"
                && options[1].label == "claude-3"
        ));
    }
}
