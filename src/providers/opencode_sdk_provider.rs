use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use futures_util::StreamExt;
use tokio::sync::mpsc;
use tokio::sync::Notify;
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
    /// Per-task cancellation for the spawned reader task. Signalled before
    /// a new reader is spawned in `subscribe_and_spawn()` to stop the old
    /// reader from persisting events for a stale session.
    reader_cancellation: Mutex<Option<Arc<Notify>>>,
    /// User id used to key the pool. May be `None` for one-shot operations
    /// (`get_models_list`, `one_shot_prompt`, title generation) which
    /// spawn transient servers outside the pool.
    user_id: Mutex<Option<Uuid>>,
    /// Working dir for the task. The pooled server's cwd is the temp config
    /// dir, not the task worktree, so the worktree is routed per HTTP call as
    /// the `directory` query param (see `start()`). Stored for diagnostics
    /// and the directory-scoped client handle.
    working_dir: Mutex<Option<PathBuf>>,
    /// Whether we will trace log every line that comes into the OpenCode SDK client as INFO
    log_data: bool,
    /// OFM footprint directory used to template the `external_directory` allowlist
    /// in the opencode server config. Empty means no restriction (backwards compat).
    footprint: PathBuf,
}

impl OpenCodeSdkProvider {
    pub async fn new(
        config: &HarnessConfig,
        config_root: &Path,
        log_data: bool,
        footprint: &Path,
    ) -> Result<Self, ProviderError> {
        let cfg_dir = ProviderConfigDir::new(config_root);
        let provider_cfg = cfg_dir.load_provider_config(&config.provider_config_ref)?;
        Ok(Self {
            config: config.clone(),
            provider_snippet: provider_cfg.raw_snippet,
            config_root: config_root.to_path_buf(),
            client: Mutex::new(None),
            session_id: Mutex::new(None),
            event_cancellation: Mutex::new(None),
            reader_cancellation: Mutex::new(None),
            user_id: Mutex::new(None),
            working_dir: Mutex::new(None),
            log_data,
            footprint: footprint.to_path_buf(),
        })
    }

    /// Set the user id used for pool lookup. Must be called before
    /// `start()`. Set by the registry when the caller passes `user_id`
    /// through `resolve_provider_for_user`.
    pub fn set_user_id(&self, user_id: Uuid) {
        *self.user_id.lock().unwrap() = Some(user_id);
    }

    fn cancel_inflight(&self) {
        if let Some(notify) = self.reader_cancellation.lock().unwrap().take() {
            notify.notify_one();
        }
        if let Some(cancellation) = self.event_cancellation.lock().unwrap().take() {
            cancellation.cancel();
        }
    }

    fn build_server_config(&self) -> serde_json::Value {
        let ext_dir = if self.footprint.as_os_str().is_empty() {
            serde_json::json!("allow")
        } else {
            let fp = self.footprint.to_string_lossy();
            serde_json::json!({
                format!("{fp}/worktrees/**"): "allow",
                format!("{fp}/archive/**"): "allow",
                "/tmp/**": "allow"
            })
        };
        let mut base = serde_json::json!({
            "provider": {},
            "permission": {
                "edit": "allow",
                "bash": "allow",
                "webfetch": "allow",
                "doom_loop": "allow",
                "external_directory": ext_dir
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
    async fn spawn_transient(
        &self,
    ) -> Result<(OpencodeClient, opencode_sdk::OpenCodeServer), ProviderError> {
        let server_config = self.build_server_config();
        let options = ServerOptions {
            config: Some(server_config),
            ..Default::default()
        };
        let (client, server) = opencode_sdk::create_opencode(options, self.log_data)
            .await
            .map_err(|e| ProviderError::Protocol(e.to_string()))?;
        Ok((client, server))
    }

    async fn subscribe_and_spawn(
        &self,
        client: &OpencodeClient,
        session_id: &str,
        user_id: Uuid,
    ) -> Result<mpsc::Receiver<ProviderEvent>, ProviderError> {
        tracing::info!(
            session_id = %session_id,
            "Subscribing to opencode global event stream"
        );

        // Cancel any previous reader task and event subscription before creating new ones
        self.cancel_inflight();

        let reader_stop: Arc<Notify> = Arc::new(Notify::new());
        *self.reader_cancellation.lock().unwrap() = Some(reader_stop.clone());

        let event_stream = client
            .event
            .subscribe()
            .await
            .map_err(|e| ProviderError::Protocol(e.to_string()))?;
        tracing::info!(session_id = %session_id, "Subscribed to opencode event stream");

        let cancellation = event_stream.cancellation_handle();
        *self.event_cancellation.lock().unwrap() = Some(cancellation);

        let s_id = session_id.to_string();
        let (tx, rx) = mpsc::channel(1024);

        tx.send(ProviderEvent::SessionStart {
            session_id: s_id.clone(),
        })
        .await
        .map_err(|_| ProviderError::Protocol("channel closed".into()))?;

        tokio::spawn(async move {
            let mut stream = event_stream;
            let pool = OpenCodeServerPool::instance();
            let mut refresh_ticker = tokio::time::interval(Duration::from_secs(5 * 60));
            refresh_ticker.tick().await; // skip the first immediate tick

            tracing::info!(session_id = %s_id, "Event reader task started");
            'reader: loop {
                tokio::select! {
                    result = stream.next() => {
                        match result {
                            Some(Ok(global)) => {
                                tracing::debug!(
                                    session_id = %s_id,
                                    event = ?global.payload,
                                    "SDK event received"
                                );
                                for provider_event in
                                    map_sdk_event_to_provider_event(&global, &s_id)
                                {
                                    if tx.send(provider_event).await.is_err() {
                                        tracing::info!(
                                            session_id = %s_id,
                                            "Event channel closed by receiver, exiting reader task"
                                        );
                                        break 'reader;
                                    }
                                }
                            }
                            Some(Err(e)) => {
                                tracing::warn!(
                                    session_id = %s_id,
                                    error = %e,
                                    "Event stream error"
                                );
                                let _ = tx
                                    .send(ProviderEvent::Error {
                                        error: e.to_string(),
                                        timestamp: chrono::Utc::now().naive_utc(),
                                    })
                                    .await;
                                break;
                            }
                            None => break,
                        }
                    }
                    _ = refresh_ticker.tick() => {
                        pool.update_timestamp(user_id).await;
                    }
                    _ = reader_stop.notified() => {
                        tracing::info!(session_id = %s_id, "Event reader task cancelled");
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

pub(crate) fn map_sdk_event_to_provider_event(
    global: &GlobalEvent,
    session_id: &str,
) -> Vec<ProviderEvent> {
    let now = chrono::Utc::now().naive_utc();
    match &global.payload {
        Event::MessagePartUpdated(data) => {
            if data.session_id != *session_id {
                return Vec::new();
            }
            match &data.part {
                Part::Text(t) => {
                    let text = t.text.trim().to_string();
                    if text.is_empty() {
                        Vec::new()
                    } else {
                        vec![ProviderEvent::Text {
                            text,
                            timestamp: now,
                        }]
                    }
                }
                Part::Reasoning(r) => {
                    let text = r.text.trim().to_string();
                    if text.is_empty() {
                        return Vec::new();
                    }
                    vec![ProviderEvent::Thinking {
                        thinking: text,
                        timestamp: now,
                    }]
                }
                Part::Tool(tool_part) => match &tool_part.state {
                    ToolState::Running(_) => {
                        let input = tool_part.input.clone().unwrap_or(serde_json::Value::Null);
                        if input.is_null() {
                            Vec::new()
                        } else {
                            vec![ProviderEvent::ToolUse {
                                tool_name: tool_part.tool.clone(),
                                tool_use_id: Some(tool_part.call_id.clone()),
                                input,
                                result: None,
                                message_id: data.message_id.clone(),
                                timestamp: now,
                            }]
                        }
                    }
                    ToolState::Completed(state) => {
                        let output = state.output.trim().to_string();
                        if output == "null" || output.is_empty() {
                            return Vec::new();
                        }
                        let input = tool_part.input.clone().unwrap_or(serde_json::Value::Null);
                        vec![ProviderEvent::ToolUse {
                            tool_name: tool_part.tool.clone(),
                            tool_use_id: Some(tool_part.call_id.clone()),
                            input,
                            result: Some(output),
                            message_id: data.message_id.clone(),
                            timestamp: now,
                        }]
                    }
                    ToolState::Error(state) => vec![ProviderEvent::Error {
                        error: state.error.clone(),
                        timestamp: now,
                    }],
                    ToolState::Pending(_) => Vec::new(),
                },
                _ => Vec::new(),
            }
        }
        Event::SessionStatus(data) => {
            if data.session_id != *session_id {
                return Vec::new();
            }
            match data.status.status_type.as_str() {
                "error" => vec![ProviderEvent::Error {
                    error: "session error".into(),
                    timestamp: now,
                }],
                "idle" => vec![ProviderEvent::Done {
                    data: serde_json::json!({}),
                    timestamp: now,
                }],
                _ => Vec::new(),
            }
        }
        Event::SessionIdle(data) => {
            if data.session_id != *session_id {
                return Vec::new();
            }
            vec![ProviderEvent::Done {
                data: serde_json::json!({}),
                timestamp: now,
            }]
        }
        Event::SessionError(data) => {
            if data.session_id != *session_id {
                return Vec::new();
            }
            vec![ProviderEvent::Error {
                error: data.error_message(),
                timestamp: now,
            }]
        }
        Event::ServerConnected(_) => vec![ProviderEvent::Ready],
        Event::ServerHeartbeat(_) => Vec::new(),
        Event::PluginAdded(_) => Vec::new(),
        Event::ReferenceUpdated(_) => Vec::new(),
        Event::IntegrationUpdated(_) => Vec::new(),
        Event::CatalogUpdated(_) => Vec::new(),
        Event::MessagePartDelta(_) => Vec::new(),
        Event::QuestionAsked(data) => {
            if data.session_id != *session_id {
                return Vec::new();
            }
            vec![ProviderEvent::QuestionAsked {
                session_id: data.session_id.clone(),
                questions: data
                    .questions
                    .iter()
                    .map(|q| crate::providers::types::AskedQuestion {
                        question: q.question.clone(),
                        header: q.header.clone(),
                        options: q
                            .options
                            .iter()
                            .map(|o| crate::providers::types::QuestionOption {
                                label: o.label.clone(),
                                description: o.description.clone(),
                            })
                            .collect(),
                    })
                    .collect(),
                tool_call_id: None,
                message_id: None,
                timestamp: now,
            }]
        }
        Event::MessageUpdated(data) => {
            if data.session_id != *session_id {
                return Vec::new();
            }
            let mut events = Vec::new();
            let parts = data
                .parts
                .as_ref()
                .filter(|p| !p.is_empty())
                .unwrap_or(&data.info.parts);
            for part in parts {
                match part {
                    Part::Text(t) => {
                        let text = t.text.trim().to_string();
                        if !text.is_empty() {
                            events.push(ProviderEvent::Text {
                                text,
                                timestamp: now,
                            });
                        }
                    }
                    Part::Tool(tool_part) => {
                        if let ToolState::Completed(state) = &tool_part.state {
                            let output = state.output.trim().to_string();
                            if output == "null" || output.is_empty() {
                                continue;
                            }
                            let input = tool_part.input.clone().unwrap_or(serde_json::Value::Null);
                            events.push(ProviderEvent::ToolUse {
                                tool_name: tool_part.tool.clone(),
                                tool_use_id: Some(tool_part.call_id.clone()),
                                input,
                                result: Some(output),
                                message_id: Some(data.info.id.clone()),
                                timestamp: now,
                            });
                        }
                    }
                    _ => {}
                }
            }
            events
        }
        _ => Vec::new(),
    }
}

#[async_trait]
impl LlmProvider for OpenCodeSdkProvider {
    async fn get_models_list(&self) -> Result<Vec<String>, ProviderError> {
        let (client, mut server) = self.spawn_transient().await?;

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
            .flat_map(|p| p.models.into_keys())
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
            .get_or_spawn(
                user_id,
                &self.config,
                &self.config_root,
                self.log_data,
                &self.footprint,
            )
            .await?;
        // Scope the client to this task's worktree so every workspace-scoped
        // HTTP call (`session.create`, `event.subscribe`, `prompt_async`,
        // `abort`) routes to the worktree via the `directory` query param.
        // The pooled server itself keeps a shared (unscoped) cwd: one server
        // cannot serve per-task directories, so routing happens per call —
        // mirroring the reference implementation. `with_directory` uses
        // `Arc::make_mut`, so only this provider's handle is affected.
        let mut client = client;
        if !working_dir.as_os_str().is_empty() {
            client = client.with_directory(&working_dir.to_string_lossy());
        }
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

        let user_id = self
            .user_id
            .lock()
            .unwrap()
            .ok_or_else(|| ProviderError::Protocol("user_id not set on provider".into()))?;

        // Subscribe to the global event stream BEFORE issuing the prompt so
        // we don't miss events that fire immediately when the prompt is
        // queued on the server.
        let rx = self
            .subscribe_and_spawn(&client, &session.id, user_id)
            .await?;

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

        let prompt = input
            .messages
            .as_array()
            .and_then(|msgs| msgs.last())
            .and_then(|last| {
                last.get("text")
                    .and_then(|t| t.as_str())
                    .or_else(|| last.get("delta").and_then(|d| d.as_str()))
            })
            .map(|s| s.to_string())
            .unwrap_or_else(|| "continue".to_string());

        let user_id = self
            .user_id
            .lock()
            .unwrap()
            .ok_or_else(|| ProviderError::Protocol("user_id not set on provider".into()))?;

        // Subscribe BEFORE issuing the prompt_async so we don't miss events
        // that fire immediately when the prompt is queued on the server.
        let rx = self
            .subscribe_and_spawn(&client, &session_id, user_id)
            .await?;

        let body =
            self.build_prompt_body(&prompt, self.config.model.as_deref().unwrap_or("default"));
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
        self.cancel_inflight();
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
        tracing::info!(
            prompt_len = prompt.len(),
            prompt_preview = %prompt.chars().take(120).collect::<String>(),
            model = %model,
            "one_shot_prompt: spawning transient server"
        );

        let (client, mut server) = self.spawn_transient().await.map_err(|e| {
            tracing::warn!(error = %e, "one_shot_prompt: spawn_transient failed");
            e
        })?;
        tracing::info!("one_shot_prompt: transient server started");

        let provider_id = self
            .extract_provider_id()
            .unwrap_or_else(|| "default".to_string());
        let config = opencode_sdk::conversation::OneShotConfig {
            model: model.to_string(),
            provider_id,
            ..Default::default()
        };

        let result = opencode_sdk::conversation::one_shot(&client, prompt, &config)
            .await
            .map_err(|e| {
                tracing::warn!(error = %e, "one_shot_prompt: one_shot call failed");
                ProviderError::Protocol(e.to_string())
            })?;

        tracing::info!(
            result_len = result.len(),
            result_preview = %result.chars().take(200).collect::<String>(),
            "one_shot_prompt: shutting down transient server"
        );

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
        self.cancel_inflight();
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
                session_id: "sess1".into(),
                part: Part::Text(TextPart {
                    text: "Hello".into(),
                }),
                delta: Some("Hello".into()),
                message_id: None,
            }),
        };
        let events = map_sdk_event_to_provider_event(&global, "sess1");
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], ProviderEvent::Text { text, .. } if text == "Hello"));
    }

    #[test]
    fn test_event_mapping_thinking() {
        let global = GlobalEvent {
            id: None,

            payload: Event::MessagePartUpdated(MessagePartUpdatedData {
                session_id: "sess1".into(),
                part: Part::Reasoning(ReasoningPart {
                    text: "thinking...".into(),
                    signature: None,
                }),
                delta: Some("thinking...".into()),
                message_id: None,
            }),
        };
        let events = map_sdk_event_to_provider_event(&global, "sess1");
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], ProviderEvent::Thinking { thinking, .. } if thinking == "thinking...")
        );
    }

    #[test]
    fn test_event_mapping_tool_use() {
        let global = GlobalEvent {
            id: None,

            payload: Event::MessagePartUpdated(MessagePartUpdatedData {
                session_id: "sess1".into(),
                part: Part::Tool(ToolPart {
                    tool: "read".into(),
                    call_id: "call1".into(),
                    state: ToolState::Running(ToolStateRunning {
                        input: serde_json::json!({"path": "/tmp"}),
                    }),
                    input: Some(serde_json::json!({"path": "/tmp"})),
                }),
                delta: None,
                message_id: None,
            }),
        };
        let events = map_sdk_event_to_provider_event(&global, "sess1");
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], ProviderEvent::ToolUse { tool_name, .. } if tool_name == "read")
        );
    }

    #[test]
    fn test_event_mapping_tool_result() {
        let global = GlobalEvent {
            id: None,

            payload: Event::MessagePartUpdated(MessagePartUpdatedData {
                session_id: "sess1".into(),
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
                message_id: None,
            }),
        };
        let events = map_sdk_event_to_provider_event(&global, "sess1");
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], ProviderEvent::ToolUse { result, .. } if result == &Some("file content".to_string()))
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
        let events = map_sdk_event_to_provider_event(&global, "sess1");
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], ProviderEvent::Error { error, .. } if error == "something went wrong")
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
        let events = map_sdk_event_to_provider_event(&global, "sess1");
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], ProviderEvent::Done { .. }));
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
        let events = map_sdk_event_to_provider_event(&global, "sess1");
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], ProviderEvent::Done { .. }));
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
        let events = map_sdk_event_to_provider_event(&global, "sess1");
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], ProviderEvent::Ready));
    }

    #[test]
    fn test_event_mapping_wrong_session_filtered() {
        let global = GlobalEvent {
            id: None,

            payload: Event::SessionIdle(SessionIdData {
                session_id: "other-session".into(),
            }),
        };
        let events = map_sdk_event_to_provider_event(&global, "sess1");
        assert!(events.is_empty());
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
            reader_cancellation: Mutex::new(None),
            user_id: Mutex::new(None),
            working_dir: Mutex::new(None),
            log_data: false,
            footprint: PathBuf::default(),
        };
        assert_eq!(provider.extract_provider_id(), Some("anthropic".into()));
    }

    #[test]
    fn test_event_mapping_returns_none() {
        let cases = vec![
            Event::ServerHeartbeat(serde_json::json!({})),
            Event::PluginAdded(serde_json::json!({"id": "sap-ai-core"})),
            Event::ReferenceUpdated(serde_json::json!({})),
            Event::IntegrationUpdated(serde_json::json!({})),
            Event::CatalogUpdated(serde_json::json!({})),
            Event::MessagePartDelta(serde_json::json!({
                "sessionID": "sess1",
                "messageID": "msg1",
                "partID": "part1",
                "field": "text",
                "delta": " files"
            })),
        ];
        for payload in cases {
            let global = GlobalEvent { id: None, payload };
            assert!(map_sdk_event_to_provider_event(&global, "sess1").is_empty());
        }
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
            reader_cancellation: Mutex::new(None),
            user_id: Mutex::new(None),
            working_dir: Mutex::new(None),
            log_data: false,
            footprint: PathBuf::default(),
        };
        assert_eq!(provider.extract_provider_id(), None);
    }

    #[test]
    fn test_filter_message_part_updated_wrong_session() {
        let global = GlobalEvent {
            id: None,
            payload: Event::MessagePartUpdated(MessagePartUpdatedData {
                session_id: "other-session".into(),
                part: Part::Text(TextPart {
                    text: "Hello".into(),
                }),
                delta: Some("Hello".into()),
                message_id: None,
            }),
        };
        let events = map_sdk_event_to_provider_event(&global, "sess1");
        assert!(events.is_empty());
    }

    #[test]
    fn test_filter_message_part_updated_correct_session() {
        let global = GlobalEvent {
            id: None,
            payload: Event::MessagePartUpdated(MessagePartUpdatedData {
                session_id: "sess1".into(),
                part: Part::Text(TextPart {
                    text: "Hello".into(),
                }),
                delta: Some("Hello".into()),
                message_id: None,
            }),
        };
        let events = map_sdk_event_to_provider_event(&global, "sess1");
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], ProviderEvent::Text { text, .. } if text == "Hello"));
    }

    #[test]
    fn test_filter_question_asked_wrong_session() {
        let global = GlobalEvent {
            id: None,
            payload: Event::QuestionAsked(QuestionAskedData {
                session_id: "other-session".into(),
                questions: vec![],
            }),
        };
        let events = map_sdk_event_to_provider_event(&global, "sess1");
        assert!(events.is_empty());
    }

    #[test]
    fn test_filter_message_updated_wrong_session() {
        let global = GlobalEvent {
            id: None,
            payload: Event::MessageUpdated(MessageUpdatedData {
                session_id: "other-session".into(),
                info: AssistantMessage {
                    id: "msg1".into(),
                    session_id: "other-session".into(),
                    time: serde_json::Value::Null,
                    error: None,
                    parent_id: None,
                    model_id: None,
                    provider_id: None,
                    mode: None,
                    path: None,
                    cost: None,
                    tokens: None,
                    finish: None,
                    parts: vec![],
                },
                parts: None,
            }),
        };
        let events = map_sdk_event_to_provider_event(&global, "sess1");
        assert!(events.is_empty());
    }

    #[test]
    fn test_event_mapping_message_updated_with_text() {
        let global = GlobalEvent {
            id: None,
            payload: Event::MessageUpdated(MessageUpdatedData {
                session_id: "sess1".into(),
                info: AssistantMessage {
                    id: "msg1".into(),
                    session_id: "sess1".into(),
                    time: serde_json::Value::Null,
                    error: None,
                    parent_id: None,
                    model_id: None,
                    provider_id: None,
                    mode: None,
                    path: None,
                    cost: None,
                    tokens: None,
                    finish: Some("stop".into()),
                    parts: vec![
                        Part::Text(TextPart {
                            text: "Hello world".into(),
                        }),
                        Part::Reasoning(ReasoningPart {
                            text: "thinking...".into(),
                            signature: None,
                        }),
                    ],
                },
                parts: None,
            }),
        };
        let events = map_sdk_event_to_provider_event(&global, "sess1");
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], ProviderEvent::Text { text, .. } if text == "Hello world"));
    }

    #[test]
    fn test_event_mapping_message_updated_with_no_text() {
        let global = GlobalEvent {
            id: None,
            payload: Event::MessageUpdated(MessageUpdatedData {
                session_id: "sess1".into(),
                info: AssistantMessage {
                    id: "msg1".into(),
                    session_id: "sess1".into(),
                    time: serde_json::Value::Null,
                    error: None,
                    parent_id: None,
                    model_id: None,
                    provider_id: None,
                    mode: None,
                    path: None,
                    cost: None,
                    tokens: None,
                    finish: Some("stop".into()),
                    parts: vec![
                        Part::Reasoning(ReasoningPart {
                            text: "thinking...".into(),
                            signature: None,
                        }),
                        Part::Tool(ToolPart {
                            tool: "read".into(),
                            call_id: "call1".into(),
                            state: ToolState::Completed(ToolStateCompleted {
                                input: serde_json::json!({"path": "/tmp"}),
                                output: "file content".into(),
                            }),
                            input: Some(serde_json::json!({"path": "/tmp"})),
                        }),
                    ],
                },
                parts: None,
            }),
        };
        let events = map_sdk_event_to_provider_event(&global, "sess1");
        // Has finish with a tool part → should produce a merged ToolUse
        assert_eq!(events.len(), 1);
        match &events[0] {
            ProviderEvent::ToolUse {
                tool_name, result, ..
            } => {
                assert_eq!(tool_name, "read");
                assert_eq!(result, &Some("file content".into()));
            }
            _ => panic!("expected ToolUse with result"),
        }
    }

    #[test]
    fn test_event_mapping_message_updated_text_from_info_parts() {
        let global = GlobalEvent {
            id: None,
            payload: Event::MessageUpdated(MessageUpdatedData {
                session_id: "sess1".into(),
                info: AssistantMessage {
                    id: "msg1".into(),
                    session_id: "sess1".into(),
                    time: serde_json::Value::Null,
                    error: None,
                    parent_id: None,
                    model_id: None,
                    provider_id: None,
                    mode: None,
                    path: None,
                    cost: None,
                    tokens: None,
                    finish: None,
                    parts: vec![Part::Text(TextPart {
                        text: "Hello world".into(),
                    })],
                },
                parts: None,
            }),
        };
        let events = map_sdk_event_to_provider_event(&global, "sess1");
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], ProviderEvent::Text { text, .. } if text == "Hello world"));
    }

    #[test]
    fn test_event_mapping_message_updated_uses_top_level_parts() {
        let global = GlobalEvent {
            id: None,
            payload: Event::MessageUpdated(MessageUpdatedData {
                session_id: "sess1".into(),
                info: AssistantMessage {
                    id: "msg1".into(),
                    session_id: "sess1".into(),
                    time: serde_json::Value::Null,
                    error: None,
                    parent_id: None,
                    model_id: None,
                    provider_id: None,
                    mode: None,
                    path: None,
                    cost: None,
                    tokens: None,
                    finish: Some("stop".into()),
                    parts: vec![],
                },
                parts: Some(vec![Part::Text(TextPart {
                    text: "from top-level parts".into(),
                })]),
            }),
        };
        let events = map_sdk_event_to_provider_event(&global, "sess1");
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], ProviderEvent::Text { text, .. } if text == "from top-level parts")
        );
    }

    #[test]
    fn test_running_with_null_input_suppressed() {
        let global = GlobalEvent {
            id: None,
            payload: Event::MessagePartUpdated(MessagePartUpdatedData {
                session_id: "sess1".into(),
                part: Part::Tool(ToolPart {
                    tool: "read".into(),
                    call_id: "call1".into(),
                    state: ToolState::Running(ToolStateRunning {
                        input: serde_json::Value::Null,
                    }),
                    input: None,
                }),
                delta: None,
                message_id: None,
            }),
        };
        let events = map_sdk_event_to_provider_event(&global, "sess1");
        assert!(
            events.is_empty(),
            "Running with null input should be suppressed"
        );
    }

    #[test]
    fn test_running_with_input_emits_tool_use() {
        let global = GlobalEvent {
            id: None,
            payload: Event::MessagePartUpdated(MessagePartUpdatedData {
                session_id: "sess1".into(),
                part: Part::Tool(ToolPart {
                    tool: "read".into(),
                    call_id: "call1".into(),
                    state: ToolState::Running(ToolStateRunning {
                        input: serde_json::json!({"path": "/tmp"}),
                    }),
                    input: Some(serde_json::json!({"path": "/tmp"})),
                }),
                delta: None,
                message_id: None,
            }),
        };
        let events = map_sdk_event_to_provider_event(&global, "sess1");
        assert_eq!(events.len(), 1);
        match &events[0] {
            ProviderEvent::ToolUse {
                tool_name,
                tool_use_id,
                input,
                result,
                ..
            } => {
                assert_eq!(tool_name, "read");
                assert_eq!(tool_use_id, &Some("call1".into()));
                assert_eq!(input, &serde_json::json!({"path": "/tmp"}));
                assert!(result.is_none(), "Running should not have result");
            }
            _ => panic!("expected ToolUse"),
        }
    }

    #[test]
    fn test_completed_emits_merged_tool_use() {
        let global = GlobalEvent {
            id: None,
            payload: Event::MessagePartUpdated(MessagePartUpdatedData {
                session_id: "sess1".into(),
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
                message_id: None,
            }),
        };
        let events = map_sdk_event_to_provider_event(&global, "sess1");
        assert_eq!(events.len(), 1);
        match &events[0] {
            ProviderEvent::ToolUse {
                tool_name,
                tool_use_id,
                input,
                result,
                ..
            } => {
                assert_eq!(tool_name, "read");
                assert_eq!(tool_use_id, &Some("call1".into()));
                assert_eq!(input, &serde_json::json!({"path": "/tmp"}));
                assert_eq!(result, &Some("file content".into()));
            }
            _ => panic!("expected ToolUse with result"),
        }
    }

    #[test]
    fn test_message_updated_with_tool_part_emits_merged_tool_use() {
        let global = GlobalEvent {
            id: None,
            payload: Event::MessageUpdated(MessageUpdatedData {
                session_id: "sess1".into(),
                info: AssistantMessage {
                    id: "msg1".into(),
                    session_id: "sess1".into(),
                    time: serde_json::Value::Null,
                    error: None,
                    parent_id: None,
                    model_id: None,
                    provider_id: None,
                    mode: None,
                    path: None,
                    cost: None,
                    tokens: None,
                    finish: Some("stop".into()),
                    parts: vec![Part::Tool(ToolPart {
                        tool: "bash".into(),
                        call_id: "call2".into(),
                        state: ToolState::Completed(ToolStateCompleted {
                            input: serde_json::json!({"command": "ls"}),
                            output: "src/\ntarget/".into(),
                        }),
                        input: Some(serde_json::json!({"command": "ls"})),
                    })],
                },
                parts: None,
            }),
        };
        let events = map_sdk_event_to_provider_event(&global, "sess1");
        assert_eq!(events.len(), 1);
        match &events[0] {
            ProviderEvent::ToolUse {
                tool_name,
                tool_use_id,
                input,
                result,
                ..
            } => {
                assert_eq!(tool_name, "bash");
                assert_eq!(tool_use_id, &Some("call2".into()));
                assert_eq!(input, &serde_json::json!({"command": "ls"}));
                assert_eq!(result, &Some("src/\ntarget/".into()));
            }
            _ => panic!("expected ToolUse with result"),
        }
    }

    #[test]
    fn test_message_updated_with_both_text_and_tool() {
        let global = GlobalEvent {
            id: None,
            payload: Event::MessageUpdated(MessageUpdatedData {
                session_id: "sess1".into(),
                info: AssistantMessage {
                    id: "msg1".into(),
                    session_id: "sess1".into(),
                    time: serde_json::Value::Null,
                    error: None,
                    parent_id: None,
                    model_id: None,
                    provider_id: None,
                    mode: None,
                    path: None,
                    cost: None,
                    tokens: None,
                    finish: Some("stop".into()),
                    parts: vec![
                        Part::Text(TextPart {
                            text: "Here is the result".into(),
                        }),
                        Part::Tool(ToolPart {
                            tool: "read".into(),
                            call_id: "call3".into(),
                            state: ToolState::Completed(ToolStateCompleted {
                                input: serde_json::json!({"path": "./src"}),
                                output: "mod.rs".into(),
                            }),
                            input: Some(serde_json::json!({"path": "./src"})),
                        }),
                    ],
                },
                parts: None,
            }),
        };
        let events = map_sdk_event_to_provider_event(&global, "sess1");
        assert_eq!(events.len(), 2);
        assert!(
            matches!(&events[0], ProviderEvent::Text { text, .. } if text == "Here is the result")
        );
        match &events[1] {
            ProviderEvent::ToolUse {
                tool_name, result, ..
            } => {
                assert_eq!(tool_name, "read");
                assert_eq!(result, &Some("mod.rs".into()));
            }
            _ => panic!("expected ToolUse with result"),
        }
    }

    #[test]
    fn test_message_part_updated_deserialization() {
        let json = r#"{
            "id": null,
            "type": "message.part.updated",
            "properties": {
                "sessionID": "ses_test123",
                "part": {
                    "type": "text",
                    "text": "Hello"
                },
                "delta": "Hello",
                "messageID": "msg1"
            }
        }"#;
        let global: GlobalEvent = serde_json::from_str(json).unwrap();
        match &global.payload {
            Event::MessagePartUpdated(data) => {
                assert_eq!(data.session_id, "ses_test123");
                assert_eq!(data.message_id, Some("msg1".into()));
            }
            _ => panic!("expected MessagePartUpdated"),
        }
    }

    #[tokio::test]
    async fn test_reader_task_cancellation() {
        let notify = Arc::new(Notify::new());
        let notify_clone = notify.clone();

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
            reader_cancellation: Mutex::new(Some(notify)),
            user_id: Mutex::new(None),
            working_dir: Mutex::new(None),
            log_data: false,
            footprint: PathBuf::default(),
        };

        provider.cancel_inflight();

        assert!(
            provider.reader_cancellation.lock().unwrap().is_none(),
            "reader_cancellation should be taken after cancel_inflight"
        );

        // The Notify should have been notified; notified() resolves immediately
        tokio::time::timeout(Duration::from_millis(100), notify_clone.notified())
            .await
            .expect("notify should be signalled within timeout");
    }
}
