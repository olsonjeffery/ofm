use std::path::{Path, PathBuf};
use std::sync::Mutex;

use async_trait::async_trait;
use futures_util::StreamExt;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::opencode_sdk::client::EventStreamCancellation;
use crate::opencode_sdk::pool::OpenCodeServerPool;
use crate::opencode_sdk::types::*;
use crate::opencode_sdk::{self, OpencodeClient, ServerOptions};
use crate::providers::config::ProviderConfigDir;
use crate::providers::types::{ProviderEvent, ResumeInput, TurnInput};
use crate::providers::{HarnessConfig, LlmProvider, ProviderError};

pub struct OpenCodeSdkProvider {
    config: HarnessConfig,
    provider_snippet: String,
    config_root: PathBuf,
    /// Set by `start()` after the pool has handed us a client for the
    /// user. Stored so other methods (`start_turn`, `resume_turn`,
    /// `abort_turn`) can use it without re-acquiring the pool.
    client: Mutex<Option<OpencodeClient>>,
    /// Last known session id — used by `abort_turn` for the best-effort
    /// `client.session.abort` call.
    session_id: Mutex<Option<String>>,
    /// Cancellation handle for the in-flight event stream reader task.
    event_cancellation: Mutex<Option<EventStreamCancellation>>,
    /// User id used to key the pool. May be `None` for one-shot operations
    /// (`get_models_list`, `one_shot_prompt`, title generation) which
    /// spawn transient servers outside the pool.
    user_id: Mutex<Option<Uuid>>,
    /// Working dir threaded through `start()` for diagnostics; the pooled
    /// server's cwd is the temp config dir, not the task worktree (the
    /// reference passes `directory` per HTTP call instead).
    working_dir: Mutex<Option<PathBuf>>,
}

impl OpenCodeSdkProvider {
    pub async fn new(config: &HarnessConfig, config_root: &Path) -> Result<Self, ProviderError> {
        let cfg_dir = ProviderConfigDir::new(config_root);
        let provider_cfg = cfg_dir.load_provider_config(&config.provider_config_ref)?;
        Ok(Self {
            config: config.clone(),
            provider_snippet: provider_cfg.raw_snippet,
            config_root: config_root.to_path_buf(),
            client: Mutex::new(None),
            session_id: Mutex::new(None),
            event_cancellation: Mutex::new(None),
            user_id: Mutex::new(None),
            working_dir: Mutex::new(None),
        })
    }

    /// Set the user id used for pool lookup. Must be called before
    /// `start()`. Set by the registry when the caller passes `user_id`
    /// through `resolve_provider_for_user`.
    pub fn set_user_id(&self, user_id: Uuid) {
        *self.user_id.lock().unwrap() = Some(user_id);
    }

    fn build_server_config(&self) -> serde_json::Value {
        let mut base = serde_json::json!({
            "provider": {},
            "permission": {
                "edit": "allow",
                "bash": "allow",
                "webfetch": "allow",
                "doom_loop": "allow",
                "external_directory": "allow"
            }
        });
        if let Ok(snippet) = serde_json::from_str::<serde_json::Value>(&self.provider_snippet) {
            deep_merge(&mut base, &snippet);
        }
        base
    }

    fn build_prompt_body(&self, prompt: &str, model: &str) -> PromptBody {
        let provider_id = self
            .extract_provider_id()
            .unwrap_or_else(|| "default".to_string());
        PromptBody {
            message_id: None,
            model: Some(ModelRef {
                provider_id,
                model_id: model.to_string(),
            }),
            agent: None,
            no_reply: None,
            system: None,
            tools: None,
            parts: vec![PartInput::Text(TextPartInput {
                text: prompt.to_string(),
            })],
        }
    }

    fn extract_provider_id(&self) -> Option<String> {
        let v: serde_json::Value = serde_json::from_str(&self.provider_snippet).ok()?;
        v.get("provider")?.as_object()?.keys().next().cloned()
    }

    /// Internal: spawn a transient server+client pair for one-shot
    /// operations (`get_models_list`, `one_shot_prompt`). The server is
    /// shut down within the caller; it does NOT participate in the pool.
    async fn spawn_transient(&self) -> Result<(OpencodeClient, opencode_sdk::OpenCodeServer), ProviderError> {
        let server_config = self.build_server_config();
        let options = ServerOptions {
            config: Some(server_config),
            ..Default::default()
        };
        let (client, server) = opencode_sdk::create_opencode(options)
            .await
            .map_err(|e| ProviderError::Protocol(e.to_string()))?;
        Ok((client, server))
    }

    async fn subscribe_and_spawn(
        &self,
        client: &OpencodeClient,
        session_id: &str,
    ) -> Result<mpsc::Receiver<ProviderEvent>, ProviderError> {
        tracing::info!(
            session_id = %session_id,
            "Subscribing to opencode global event stream"
        );
        let event_stream = client
            .event
            .subscribe()
            .await
            .map_err(|e| ProviderError::Protocol(e.to_string()))?;
        tracing::info!(session_id = %session_id, "Subscribed to opencode event stream");

        let cancellation = event_stream.cancellation_handle();
        *self.event_cancellation.lock().unwrap() = Some(cancellation);

        let (tx, rx) = mpsc::channel(1024);
        let s_id = session_id.to_string();

        tx.send(ProviderEvent::SessionStart {
            session_id: session_id.to_string(),
        })
        .await
        .map_err(|_| ProviderError::Protocol("channel closed".into()))?;

        tokio::spawn(async move {
            let mut stream = event_stream;
            tracing::info!(session_id = %s_id, "Event reader task started");
            while let Some(result) = stream.next().await {
                match result {
                    Ok(global) => {
                        tracing::debug!(
                            session_id = %s_id,
                            event = ?global.payload,
                            "SDK event received"
                        );
                        if let Some(provider_event) =
                            map_sdk_event_to_provider_event(&global, &s_id)
                        {
                            if tx.send(provider_event).await.is_err() {
                                tracing::info!(
                                    session_id = %s_id,
                                    "Event channel closed by receiver, exiting reader task"
                                );
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            session_id = %s_id,
                            error = %e,
                            "Event stream error"
                        );
                        let _ = tx
                            .send(ProviderEvent::Error {
                                error: e.to_string(),
                            })
                            .await;
                        break;
                    }
                }
            }
            tracing::info!(session_id = %s_id, "Event reader task exited");
        });

        Ok(rx)
    }
}

fn deep_merge(base: &mut serde_json::Value, overlay: &serde_json::Value) {
    match (base, overlay) {
        (serde_json::Value::Object(base_map), serde_json::Value::Object(overlay_map)) => {
            for (key, val) in overlay_map {
                if base_map.contains_key(key) {
                    deep_merge(&mut base_map[key], val);
                } else {
                    base_map.insert(key.clone(), val.clone());
                }
            }
        }
        (base, overlay) => *base = overlay.clone(),
    }
}

fn map_sdk_event_to_provider_event(
    global: &GlobalEvent,
    session_id: &str,
) -> Option<ProviderEvent> {
    match &global.payload {
        Event::MessagePartUpdated(data) => match &data.part {
            Part::Text(t) => Some(ProviderEvent::TextChunk {
                delta: data.delta.clone().unwrap_or_else(|| t.text.clone()),
            }),
            Part::Reasoning(r) => Some(ProviderEvent::Thinking {
                thinking: r.text.clone(),
            }),
            Part::Tool(tool_part) => match &tool_part.state {
                ToolState::Running(_) => Some(ProviderEvent::ToolUse {
                    tool_name: tool_part.tool.clone(),
                    tool_use_id: Some(tool_part.call_id.clone()),
                    input: tool_part.input.clone().unwrap_or(serde_json::Value::Null),
                }),
                ToolState::Completed(state) => Some(ProviderEvent::ToolResult {
                    tool_use_id: Some(tool_part.call_id.clone()),
                    result: state.output.clone(),
                }),
                ToolState::Error(state) => Some(ProviderEvent::Error {
                    error: state.error.clone(),
                }),
                ToolState::Pending(_) => None,
            },
            _ => None,
        },
        Event::SessionStatus(data) => {
            if data.session_id == session_id {
                if data.status.status_type == "error" {
                    Some(ProviderEvent::Error {
                        error: "session error".into(),
                    })
                } else if data.status.status_type == "idle" {
                    Some(ProviderEvent::Done(serde_json::json!({})))
                } else {
                    None
                }
            } else {
                None
            }
        }
        Event::SessionIdle(data) => {
            if data.session_id == session_id {
                Some(ProviderEvent::Done(serde_json::json!({})))
            } else {
                None
            }
        }
        Event::SessionError(data) => {
            if data.session_id == session_id {
                Some(ProviderEvent::Error {
                    error: data.error_message(),
                })
            } else {
                None
            }
        }
        Event::ServerConnected(_) => Some(ProviderEvent::Ready),
        _ => None,
    }
}

#[async_trait]
impl LlmProvider for OpenCodeSdkProvider {
    async fn get_models_list(&self) -> Result<Vec<String>, ProviderError> {
        let server_config = self.build_server_config();
        let options = ServerOptions {
            config: Some(server_config),
            ..Default::default()
        };
        let (client, mut server) = opencode_sdk::create_opencode(options)
            .await
            .map_err(|e| ProviderError::Protocol(e.to_string()))?;

        let providers = client
            .config
            .providers()
            .await
            .map_err(|e| ProviderError::Protocol(e.to_string()))?;

        server
            .shutdown()
            .await
            .map_err(|e| ProviderError::Protocol(e.to_string()))?;

        let mut models: Vec<String> = providers
            .into_iter()
            .flat_map(|p| p.models.into_keys().collect::<Vec<_>>())
            .collect();
        models.sort();
        models.dedup();
        Ok(models)
    }

    async fn start(&mut self, working_dir: &Path) -> Result<(), ProviderError> {
        // Acquire a pooled opencode server for this user. The pool is a
        // process-wide singleton (see `src/opencode_sdk/pool.rs`); servers
        // are shared across all conversations for the same user and
        // persist across Stop Agent / turn completion. The provider
        // borrows the client handle (a cheap `Arc` clone) — it does NOT
        // own the server.
        let user_id = self
            .user_id
            .lock()
            .unwrap()
            .ok_or_else(|| ProviderError::Protocol("user_id not set on provider".into()))?;
        let client = OpenCodeServerPool::instance()
            .get_or_spawn(user_id, &self.config, &self.config_root)
            .await?;
        *self.client.lock().unwrap() = Some(client);
        *self.working_dir.lock().unwrap() = Some(working_dir.to_path_buf());
        Ok(())
    }

    async fn start_turn(
        &self,
        input: TurnInput,
    ) -> Result<mpsc::Receiver<ProviderEvent>, ProviderError> {
        let client = self
            .client
            .lock()
            .unwrap()
            .clone()
            .ok_or(ProviderError::NotStarted)?;

        tracing::info!(model = %input.model, "start_turn: creating opencode session");
        let session = client
            .session
            .create(&input.prompt)
            .await
            .map_err(|e| ProviderError::Protocol(e.to_string()))?;

        *self.session_id.lock().unwrap() = Some(session.id.clone());

        // Subscribe to the global event stream BEFORE issuing the prompt so
        // we don't miss events that fire immediately when the prompt is
        // queued on the server.
        let rx = self.subscribe_and_spawn(&client, &session.id).await?;

        let body = self.build_prompt_body(&input.prompt, &input.model);
        tracing::info!(
            session_id = %session.id,
            "start_turn: dispatching prompt_async"
        );
        client
            .session
            .prompt_async(&session.id, &body)
            .await
            .map_err(|e| ProviderError::Protocol(e.to_string()))?;
        tracing::info!(
            session_id = %session.id,
            "start_turn: prompt_async dispatched"
        );

        Ok(rx)
    }

    async fn resume_turn(
        &self,
        input: ResumeInput,
    ) -> Result<mpsc::Receiver<ProviderEvent>, ProviderError> {
        let client = self
            .client
            .lock()
            .unwrap()
            .clone()
            .ok_or(ProviderError::NotStarted)?;

        // Mirror the reference implementation's `sendTurnMessage` (see
        // `spec/reference/server/services/providers/opencode/index.ts`):
        // resume reuses the existing `session_id` and re-issues
        // `promptAsync` against it — `session.create` is NOT called. This
        // works because the opencode server is persistent across Stop
        // Agent / turn completion (the server is only shut down when ofm
        // exits); if the server were killed, the session_id stored in the
        // DB would be stale and resume would surface a server-side error.
        let session_id = input.session_id;
        *self.session_id.lock().unwrap() = Some(session_id.clone());

        // Extract the last user message from the transcript to use as the prompt
        let prompt = if let Some(messages) = input.messages.as_array() {
            if let Some(last) = messages.last() {
                if let Some(text) = last.get("text").and_then(|t| t.as_str()) {
                    text.to_string()
                } else if let Some(delta) = last.get("delta").and_then(|d| d.as_str()) {
                    delta.to_string()
                } else {
                    "continue".to_string()
                }
            } else {
                "continue".to_string()
            }
        } else {
            "continue".to_string()
        };

        // Subscribe BEFORE issuing the prompt_async so we don't miss events
        // that fire immediately when the prompt is queued on the server.
        let rx = self.subscribe_and_spawn(&client, &session_id).await?;

        let body = self.build_prompt_body(&prompt, self.config.model.as_deref().unwrap_or("default"));
        tracing::info!(
            session_id = %session_id,
            "resume_turn: dispatching prompt_async"
        );
        client
            .session
            .prompt_async(&session_id, &body)
            .await
            .map_err(|e| ProviderError::Protocol(e.to_string()))?;
        tracing::info!(
            session_id = %session_id,
            "resume_turn: prompt_async dispatched"
        );

        Ok(rx)
    }

    async fn abort_turn(&self) -> Result<(), ProviderError> {
        if let Some(cancellation) = self.event_cancellation.lock().unwrap().take() {
            cancellation.cancel();
        }
        let (session_id, client) = {
            let s = self.session_id.lock().unwrap().clone();
            let c = self.client.lock().unwrap().clone();
            (s, c)
        };
        if let (Some(client), Some(session_id)) = (client, session_id) {
            let _ = client.session.abort(&session_id).await;
        }
        Ok(())
    }

    async fn one_shot_prompt(&self, prompt: &str, model: &str) -> Result<String, ProviderError> {
        let server_config = self.build_server_config();
        let options = ServerOptions {
            config: Some(server_config),
            ..Default::default()
        };
        let (client, mut server) = opencode_sdk::create_opencode(options)
            .await
            .map_err(|e| ProviderError::Protocol(e.to_string()))?;

        let config = opencode_sdk::conversation::OneShotConfig {
            model: model.to_string(),
            ..Default::default()
        };

        let result = opencode_sdk::conversation::one_shot(&client, prompt, &config)
            .await
            .map_err(|e| ProviderError::Protocol(e.to_string()))?;

        let _ = server.shutdown().await;
        Ok(result)
    }

    async fn shutdown(&mut self) -> Result<bool, ProviderError> {
        // Pooled-server design: the provider does NOT own the opencode
        // subprocess, so `shutdown` only cancels the in-flight event
        // stream reader and drops the borrowed client handle. The
        // underlying `opencode serve` process stays alive in the pool;
        // it is reaped by the idle-reaper task or by the process-exit
        // handlers in `src/main.rs` (which call
        // `OpenCodeServerPool::instance().shutdown_all()`).
        if let Some(cancellation) = self.event_cancellation.lock().unwrap().take() {
            cancellation.cancel();
        }
        *self.client.lock().unwrap() = None;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_mapping_text_chunk() {
        let global = GlobalEvent {
            id: None,

            payload: Event::MessagePartUpdated(MessagePartUpdatedData {
                part: Part::Text(TextPart {
                    text: "Hello".into(),
                }),
                delta: Some("Hello".into()),
            }),
        };
        let event = map_sdk_event_to_provider_event(&global, "sess1");
        assert!(matches!(event, Some(ProviderEvent::TextChunk { delta }) if delta == "Hello"));
    }

    #[test]
    fn test_event_mapping_thinking() {
        let global = GlobalEvent {
            id: None,

            payload: Event::MessagePartUpdated(MessagePartUpdatedData {
                part: Part::Reasoning(ReasoningPart {
                    text: "thinking...".into(),
                    signature: None,
                }),
                delta: Some("thinking...".into()),
            }),
        };
        let event = map_sdk_event_to_provider_event(&global, "sess1");
        assert!(
            matches!(event, Some(ProviderEvent::Thinking { thinking }) if thinking == "thinking...")
        );
    }

    #[test]
    fn test_event_mapping_tool_use() {
        let global = GlobalEvent {
            id: None,

            payload: Event::MessagePartUpdated(MessagePartUpdatedData {
                part: Part::Tool(ToolPart {
                    tool: "read".into(),
                    call_id: "call1".into(),
                    state: ToolState::Running(ToolStateRunning {
                        input: serde_json::json!({"path": "/tmp"}),
                    }),
                    input: Some(serde_json::json!({"path": "/tmp"})),
                }),
                delta: None,
            }),
        };
        let event = map_sdk_event_to_provider_event(&global, "sess1");
        assert!(
            matches!(event, Some(ProviderEvent::ToolUse { tool_name, .. }) if tool_name == "read")
        );
    }

    #[test]
    fn test_event_mapping_tool_result() {
        let global = GlobalEvent {
            id: None,

            payload: Event::MessagePartUpdated(MessagePartUpdatedData {
                part: Part::Tool(ToolPart {
                    tool: "read".into(),
                    call_id: "call1".into(),
                    state: ToolState::Completed(ToolStateCompleted {
                        input: serde_json::json!({"path": "/tmp"}),
                        output: "file content".into(),
                    }),
                    input: Some(serde_json::json!({"path": "/tmp"})),
                }),
                delta: None,
            }),
        };
        let event = map_sdk_event_to_provider_event(&global, "sess1");
        assert!(
            matches!(event, Some(ProviderEvent::ToolResult { result, .. }) if result == "file content")
        );
    }

    #[test]
    fn test_event_mapping_session_error() {
        let global = GlobalEvent {
            id: None,
            payload: Event::SessionError(SessionErrorData {
                session_id: "sess1".into(),
                error: serde_json::json!("something went wrong"),
            }),
        };
        let event = map_sdk_event_to_provider_event(&global, "sess1");
        assert!(
            matches!(event, Some(ProviderEvent::Error { error }) if error == "something went wrong")
        );
    }

    #[test]
    fn test_event_mapping_session_idle_done() {
        let global = GlobalEvent {
            id: None,

            payload: Event::SessionIdle(SessionIdData {
                session_id: "sess1".into(),
            }),
        };
        let event = map_sdk_event_to_provider_event(&global, "sess1");
        assert!(matches!(event, Some(ProviderEvent::Done(_))));
    }

    #[test]
    fn test_event_mapping_session_status_idle_done() {
        let global = GlobalEvent {
            id: None,

            payload: Event::SessionStatus(SessionStatusData {
                session_id: "sess1".into(),
                status: SessionStatusValue {
                    status_type: "idle".into(),
                },
            }),
        };
        let event = map_sdk_event_to_provider_event(&global, "sess1");
        assert!(matches!(event, Some(ProviderEvent::Done(_))));
    }

    #[test]
    fn test_event_mapping_server_connected_ready() {
        let global = GlobalEvent {
            id: None,

            payload: Event::ServerConnected(ServerConnectedData {
                version: None,
                config: None,
            }),
        };
        let event = map_sdk_event_to_provider_event(&global, "sess1");
        assert!(matches!(event, Some(ProviderEvent::Ready)));
    }

    #[test]
    fn test_event_mapping_wrong_session_filtered() {
        let global = GlobalEvent {
            id: None,

            payload: Event::SessionIdle(SessionIdData {
                session_id: "other-session".into(),
            }),
        };
        let event = map_sdk_event_to_provider_event(&global, "sess1");
        assert!(event.is_none());
    }

    #[test]
    fn test_deep_merge_overrides() {
        let mut base = serde_json::json!({"key1": "val1", "nested": {"a": 1}});
        let overlay = serde_json::json!({"key1": "overridden", "nested": {"b": 2}});
        deep_merge(&mut base, &overlay);
        assert_eq!(base["key1"], "overridden");
        assert_eq!(base["nested"]["a"], 1);
        assert_eq!(base["nested"]["b"], 2);
    }

    #[test]
    fn test_extract_provider_id() {
        let snippet = r#"{"provider": {"anthropic": {"apiKey": "sk-..."}}}"#;
        let provider = OpenCodeSdkProvider {
            config: HarnessConfig {
                agent_type: "test".into(),
                harness: "opencode".into(),
                provider_config_ref: "test.json".into(),
                model: None,
                effort: None,
                scope: crate::db::schema::ScopeType::Global,
            },
            provider_snippet: snippet.into(),
            config_root: PathBuf::from("/tmp"),
            client: Mutex::new(None),
            session_id: Mutex::new(None),
            event_cancellation: Mutex::new(None),
            user_id: Mutex::new(None),
            working_dir: Mutex::new(None),
        };
        assert_eq!(provider.extract_provider_id(), Some("anthropic".into()));
    }

    #[test]
    fn test_extract_provider_id_empty() {
        let provider = OpenCodeSdkProvider {
            config: HarnessConfig {
                agent_type: "test".into(),
                harness: "opencode".into(),
                provider_config_ref: "test.json".into(),
                model: None,
                effort: None,
                scope: crate::db::schema::ScopeType::Global,
            },
            provider_snippet: "{}".into(),
            config_root: PathBuf::from("/tmp"),
            client: Mutex::new(None),
            session_id: Mutex::new(None),
            event_cancellation: Mutex::new(None),
            user_id: Mutex::new(None),
            working_dir: Mutex::new(None),
        };
        assert_eq!(provider.extract_provider_id(), None);
    }
}
