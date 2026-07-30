use std::path::PathBuf;
use std::str::FromStr;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::db::schema::{AgentType, Conversation, ConversationWithRun, TaskAgentRun};
use crate::providers::registry;
use crate::providers::types::{ProviderEvent, ResumeInput};
use crate::providers::HarnessConfig;
use crate::server::ws::message::{ServerMessage, TopicId, WsTopic, WsTopicKind};
use crate::server::{error::ServerError, state::AppState};
use crate::services::{session, tasks, transcript};
use futures_util::FutureExt;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Debug, Serialize)]
pub struct ConversationDetail {
    pub conversation: Conversation,
    pub run: Option<TaskAgentRun>,
    pub messages: Vec<ProviderEvent>,
}

#[derive(Debug, Deserialize)]
struct SendMessageRequest {
    text: String,
}

pub fn conversations_router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_conversations))
        .route("/{id}", get(get_conversation))
        .route("/{id}/messages", post(send_message))
}

async fn list_conversations(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(task_id): Path<i64>,
) -> Result<Json<Vec<ConversationWithRun>>, ServerError> {
    let task = tasks::get_task(&state.db, task_id)
        .await
        .map_err(|_| ServerError::NotFound("Task not found".into()))?;
    if task.user_id != auth.user_id {
        return Err(ServerError::NotFound("Task not found".into()));
    }
    let convs = tasks::list_conversations_for_task(&state.db, task_id)
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;
    Ok(Json(convs))
}

async fn get_conversation(
    auth: AuthUser,
    State(state): State<AppState>,
    Path((task_id, conv_id)): Path<(i64, Uuid)>,
) -> Result<Json<ConversationDetail>, ServerError> {
    let task = tasks::get_task(&state.db, task_id)
        .await
        .map_err(|_| ServerError::NotFound("Task not found".into()))?;
    if task.user_id != auth.user_id {
        return Err(ServerError::NotFound("Task not found".into()));
    }
    let conv = session::resume_session(&state.db, conv_id)
        .await
        .map_err(|_| ServerError::NotFound("Conversation not found".into()))?;

    if conv.task_id != task_id {
        return Err(ServerError::NotFound("Conversation not found".into()));
    }

    let run = tasks::get_agent_run_by_conversation(&state.db, &conv_id)
        .await
        .ok();

    let provider_session_id = conv.provider_session_id.clone().unwrap_or_default();
    let messages = transcript::load_transcript(&state.db, &provider_session_id, conv.task_id)
        .await
        .unwrap_or_default();

    Ok(Json(ConversationDetail {
        conversation: conv,
        run,
        messages,
    }))
}

async fn send_message(
    auth: AuthUser,
    State(state): State<AppState>,
    Path((task_id, conv_id)): Path<(i64, Uuid)>,
    Json(body): Json<SendMessageRequest>,
) -> Result<StatusCode, ServerError> {
    let task = tasks::get_task(&state.db, task_id)
        .await
        .map_err(|_| ServerError::NotFound("Task not found".into()))?;
    if task.user_id != auth.user_id {
        return Err(ServerError::NotFound("Task not found".into()));
    }
    let conv = session::resume_session(&state.db, conv_id)
        .await
        .map_err(|_| ServerError::NotFound("Conversation not found".into()))?;

    if conv.task_id != task_id {
        return Err(ServerError::NotFound("Conversation not found".into()));
    }

    let provider_session_id = conv.provider_session_id.clone().unwrap_or_default();
    tracing::info!(
        task_id = %task_id,
        conversation_id = %conv_id,
        session_id = %provider_session_id,
        "Sending message to resume session"
    );

    let _ = state.db.execute(
        "UPDATE task_agent_runs SET status = 'running', completed_at = NULL WHERE conversation_id = $1 AND status IN ('completed', 'failed')",
        hiqlite::params!(conv_id.to_string()),
    ).await;

    if body.text.trim().is_empty() {
        return Err(ServerError::BadRequest("message text is required".into()));
    }

    // Persist the user's message
    let user_ts = chrono::Utc::now().naive_utc();
    let user_event = ProviderEvent::UserText {
        text: body.text.clone(),
        timestamp: user_ts,
    };
    transcript::persist_event(&state.db, &user_event, &provider_session_id, task_id)
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;

    let now = chrono::Utc::now().naive_utc().to_string();
    let _ = state
        .db
        .execute(
            "UPDATE conversations SET updated_at = $1 WHERE id = $2",
            hiqlite::params!(&now, conv_id.to_string()),
        )
        .await;

    // Fire-and-forget title generation if conversation doesn't have one yet
    if conv.name.is_none() {
        tracing::info!(
            conversation_id = %conv_id,
            task_id = task_id,
            model = %conv.model,
            text_preview = %body.text.chars().take(120).collect::<String>(),
            "send_message: spawning title generation"
        );
        let db = state.db.clone();
        let config_root = PathBuf::from(&state.config_root);
        let harness_config = match registry::resolve_harness_config(
            &state.db,
            &AgentType::from_str(&conv.model).unwrap_or(AgentType::Implementation),
            Some(&task.user_id),
            Some(task.project_id),
        )
        .await
        {
            Ok(cfg) => {
                tracing::info!(
                    conversation_id = %conv_id,
                    provider_config_ref = %cfg.provider_config_ref,
                    "send_message: harness_config resolved from conv.model"
                );
                cfg
            }
            Err(e1) => {
                tracing::warn!(
                    conversation_id = %conv_id,
                    model = %conv.model,
                    error = %e1,
                    "send_message: resolve_harness_config failed for conv.model, trying agent_run"
                );
                let run = tasks::get_agent_run_by_conversation(&state.db, &conv_id)
                    .await
                    .ok();
                let agent_type = run
                    .as_ref()
                    .and_then(|r| AgentType::from_str(&r.agent_type.to_string()).ok())
                    .unwrap_or(AgentType::Implementation);
                tracing::info!(
                    conversation_id = %conv_id,
                    agent_type = %agent_type,
                    "send_message: retrying with agent_type from run"
                );
                registry::resolve_harness_config(
                    &state.db,
                    &agent_type,
                    Some(&task.user_id),
                    Some(task.project_id),
                )
                .await
                .unwrap_or_else(|e2| {
                    tracing::warn!(
                        conversation_id = %conv_id,
                        agent_type = %agent_type,
                        error = %e2,
                        "send_message: fallback resolve_harness_config also failed, using empty HarnessConfig"
                    );
                    HarnessConfig {
                        agent_type: agent_type.to_string(),
                        harness: "opencode".to_string(),
                        provider_config_ref: String::new(),
                        model: None,
                        effort: None,
                        scope: crate::db::schema::ScopeType::Global,
                    }
                })
            }
        };
        let _log_data = state.config.info_log_client_data;
        let _c_id = conv_id;
        let _task_id = task_id;
        let _text = body.text.clone();
        let _ws_bus = state.ws_bus.clone();
        tokio::spawn(async move {
            tracing::info!(
                conversation_id = %_c_id,
                "send_message: title generation task started"
            );
            crate::providers::generate_conversation_title(
                &db,
                &config_root,
                &harness_config,
                _c_id,
                &_text,
                _log_data,
            )
            .await;
            tracing::info!(
                conversation_id = %_c_id,
                "send_message: generate_conversation_title returned"
            );
            match crate::services::session::resume_session(&db, _c_id).await {
                Ok(conv) => {
                    tracing::info!(
                        conversation_id = %_c_id,
                        name = ?conv.name,
                        is_valid = conv.name.as_deref().map(crate::webapp::components::conversation_list::is_valid_name).unwrap_or(false),
                        "send_message: resumed conversation after title generation"
                    );
                    if let Some(ref name) = conv.name {
                        if crate::webapp::components::conversation_list::is_valid_name(name) {
                            tracing::info!(conversation_id = %_c_id, name = %name, "send_message: broadcasting conversation-name-updated");
                            _ws_bus
                                .broadcast(
                                    &WsTopic {
                                        kind: WsTopicKind::Task,
                                        id: TopicId(_task_id),
                                    },
                                    ServerMessage::Event {
                                        topic: WsTopic {
                                            kind: WsTopicKind::Task,
                                            id: TopicId(_task_id),
                                        },
                                        event_type: "conversation-name-updated".to_string(),
                                        timestamp: chrono::Utc::now(),
                                        payload: serde_json::json!({
                                            "conversation_id": _c_id.to_string(),
                                            "name": name,
                                        }),
                                        html: None,
                                    },
                                )
                                .await;
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        conversation_id = %_c_id,
                        error = %e,
                        "send_message: resume_session failed after title generation"
                    );
                }
            }
        });
    }

    // Broadcast user message via WS
    let topic = WsTopic {
        kind: WsTopicKind::Task,
        id: TopicId(task_id),
    };
    let ts_str = user_ts.format("%Y-%m-%d %H:%M:%S").to_string();
    let msg = ServerMessage::Event {
        topic: topic.clone(),
        event_type: "user_text".to_string(),
        timestamp: chrono::Utc::now(),
        payload: serde_json::json!({"text": body.text, "conversation_id": conv_id.to_string(), "timestamp": ts_str}),
        html: Some(crate::webapp::components::message_stream::render_event(
            &user_event,
        )),
    };
    state.ws_bus.broadcast(&topic, msg).await;

    let cwd = tasks::get_worktree_by_task(&state.db, task_id)
        .await
        .ok()
        .map(|w| w.worktree_path)
        .unwrap_or_else(|| "/tmp".to_string());
    let access_token = crate::orchestration::get_or_refresh_token(
        &state.db,
        &state.access_tokens,
        state.oidc_provider.as_ref(),
        task.user_id,
    )
    .await
    .unwrap_or_default();
    if !access_token.is_empty() {
        crate::orchestration::write_ofm_agent_json(
            &cwd,
            &access_token,
            &state.config.hostname,
            state.cfg_port,
        );
    }

    // Load transcript and resume the provider
    let mut sessions = state.active_sessions.lock().await;

    match sessions.remove(&conv_id.to_string()) {
        Some(p) => {
            tracing::info!(
                task_id = %task_id,
                conversation_id = %conv_id,
                session_id = %provider_session_id,
                "Found active provider, loading transcript"
            );

            let messages = transcript::load_transcript(&state.db, &provider_session_id, task_id)
                .await
                .map_err(|e| ServerError::Internal(e.to_string()))?;

            tracing::info!(
                task_id = %task_id,
                conversation_id = %conv_id,
                message_count = messages.len(),
                "Loaded transcript"
            );

            let messages_json = serde_json::to_value(&messages)
                .map_err(|e| ServerError::Internal(e.to_string()))?;

            let resume_input = ResumeInput::new(provider_session_id.clone(), messages_json);

            match p.resume_turn(resume_input).await {
                Ok(mut rx) => {
                    sessions.insert(conv_id.to_string(), p);

                    tracing::info!(
                        task_id = %task_id,
                        conversation_id = %conv_id,
                        session_id = %provider_session_id,
                        "Successfully resumed turn, spawning broadcast task"
                    );
                    let db = state.db.clone();
                    let ws_bus = state.ws_bus.clone();
                    let active_sessions = state.active_sessions.clone();
                    let access_tokens = state.access_tokens.clone();
                    let config = state.config.clone();
                    let oidc_for_spawn = state.oidc_provider.clone();
                    let s_id = provider_session_id;
                    let c_id = conv_id;
                    let config_root = PathBuf::from(&state.config_root);
                    let footprint = state.footprint.clone();
                    let archive_root = state.archive_root.clone();
                    let task_data = tasks::get_task(&state.db, task_id).await.ok();

                    tokio::spawn(async move {
                        let completed_normally = Arc::new(AtomicBool::new(false));

                        let broadcast_fut = AssertUnwindSafe(async {
                            loop {
                                tokio::select! {
                                    event = rx.recv() => {
                                        let event = match event {
                                            Some(e) => e,
                                            None => break,
                                        };

                                        let topic = WsTopic {
                                            kind: WsTopicKind::Task,
                                            id: TopicId(task_id),
                                        };

                                        // Merged ToolUse with result → update existing row instead of inserting new one
                                        let is_merged_tool = matches!(&event, ProviderEvent::ToolUse { result: Some(..), .. });

                                        if !is_merged_tool {
                                            if let Err(e) = transcript::persist_event(
                                                &db, &event, &s_id, task_id
                                            ).await {
                                                tracing::error!("Failed to persist event: {e}");
                                                let error_event = ProviderEvent::Error {
                                                    error: format!("Failed to persist event: {e}"),
                                                    timestamp: chrono::Utc::now().naive_utc(),
                                                };
                                                let (event_type, payload) = error_event.to_ws_event();
                                                let msg = ServerMessage::Event {
                                                    topic: topic.clone(),
                                                    event_type,
                                                    timestamp: chrono::Utc::now(),
                                                    payload,
                                                    html: None,
                                                };
                                                ws_bus.broadcast(&topic, msg).await;
                                                break;
                                            }
                                        } else {
                                            // Try to update existing ToolUse row with completed data
                                            let tool_use_id = match &event {
                                                ProviderEvent::ToolUse { tool_use_id: Some(id), .. } => id.clone(),
                                                _ => String::new(),
                                            };
                                            if !tool_use_id.is_empty() {
                                                match transcript::update_tool_event(
                                                    &db, &tool_use_id, &event, &s_id, task_id
                                                ).await {
                                                    Ok(false) => {
                                                        // No prior ToolUse row found — persist as new event
                                                        if let Err(e) = transcript::persist_event(
                                                            &db, &event, &s_id, task_id
                                                        ).await {
                                                            tracing::error!("Failed to persist merged tool event: {e}");
                                                        }
                                                    }
                                                    Err(e) => tracing::error!("Failed to update tool event: {e}"),
                                                    _ => {}
                                                }
                                            } else {
                                                // No tool_use_id — persist as normal event
                                                if let Err(e) = transcript::persist_event(
                                                    &db, &event, &s_id, task_id
                                                ).await {
                                                    tracing::error!("Failed to persist tool event: {e}");
                                                }
                                            }
                                        }

                                        if let ProviderEvent::SessionStart { session_id } = &event {
                                            let _ = db.execute(
                                                "UPDATE conversations SET provider_session_id = $1 WHERE id = $2",
                                                hiqlite::params!(session_id, c_id.to_string()),
                                            ).await;
                                        }

                                        let (event_type, payload) = event.to_ws_event();
                                        let is_done = matches!(event, ProviderEvent::Done { .. });

                                        let payload = if let Some(obj) = payload.as_object() {
                                            let mut map = obj.clone();
                                            map.insert("conversation_id".to_string(), serde_json::json!(c_id.to_string()));
                                            serde_json::Value::Object(map)
                                        } else {
                                            serde_json::json!({"conversation_id": c_id.to_string()})
                                        };

                                        let rendered = crate::webapp::components::message_stream::render_event(&event);
                                        let msg = ServerMessage::Event {
                                            topic: topic.clone(),
                                            event_type,
                                            timestamp: chrono::Utc::now(),
                                            payload,
                                            html: if rendered.is_empty() { None } else { Some(rendered) },
                                        };

                                        ws_bus.broadcast(&topic, msg).await;

                                        if is_done {
                                            completed_normally.store(true, Ordering::SeqCst);
                                            let done_now = chrono::Utc::now().naive_utc().to_string();
                                            let _ = db.execute(
                                                "UPDATE conversations SET updated_at = $1 WHERE id = $2",
                                                hiqlite::params!(&done_now, c_id.to_string()),
                                            ).await;
                                            match crate::orchestration::completion_handler(
                                                &db, c_id, &active_sessions, &ws_bus
                                            ).await {
                                                Ok(crate::orchestration::NextAction::StartAgent(agent_type)) => {
                                                    let db = db.clone();
                                                    let config_root = config_root.clone();
                                                    let footprint = footprint.clone();
                                                    let archive_root = archive_root.clone();
                                                    let active_sessions = active_sessions.clone();
                                                    let ws_bus = ws_bus.clone();
                                                    let access_tokens = access_tokens.clone();
                                                    let config = config.clone();
                                                    let oidc_for_spawn = oidc_for_spawn.clone();
                                                    let task_data = task_data.clone();
                                                    tokio::spawn(async move {
                                                        if let Some(task) = task_data {
                                                            if let Err(e) = crate::orchestration::start_next_agent(
                                                                &db, &task, agent_type,
                                                                &config_root, &footprint, &archive_root,
                                                                &active_sessions, &ws_bus,
                                                                &access_tokens, &oidc_for_spawn, &config,
                                                            ).await {
                                                                tracing::warn!("Failed to auto-advance to next agent: {e:?}");
                                                            }
                                                        }
                                                    });
                                                }
                                                Ok(_) => {}
                                                Err(e) => tracing::warn!("Error in completion handler: {e:?}"),
                                            }
                                            break;
                                        }
                                    }
                                }
                            }
                        });

                        let result = broadcast_fut.catch_unwind().await;
                        if let Err(ref panic) = result {
                            tracing::error!("Broadcast task panicked: {panic:?}");
                        }

                        if !completed_normally.load(Ordering::SeqCst) {
                            {
                                let sessions = active_sessions.lock().await;
                                if let Some(p) = sessions.get(&c_id.to_string()) {
                                    if let Err(e) = p.abort_turn().await {
                                        tracing::warn!(conversation_id = %c_id, "Error aborting provider in broadcast cleanup: {e}");
                                    }
                                }
                            }

                            let _ = db.execute(
                                "UPDATE task_agent_runs SET status = 'failed' WHERE conversation_id = $1",
                                hiqlite::params!(c_id.to_string()),
                            ).await;

                            let topic = WsTopic {
                                kind: WsTopicKind::Task,
                                id: TopicId(task_id),
                            };
                            let error_event = ProviderEvent::Error {
                                error:
                                    "Agent session ended unexpectedly. Send a message to resume."
                                        .into(),
                                timestamp: chrono::Utc::now().naive_utc(),
                            };
                            let msg = ServerMessage::Event {
                                topic: topic.clone(),
                                event_type: "error".to_string(),
                                timestamp: chrono::Utc::now(),
                                payload: serde_json::json!({"error": "Agent session ended unexpectedly. Send a message to resume.", "conversation_id": c_id.to_string()}),
                                html: Some(
                                    crate::webapp::components::message_stream::render_event(
                                        &error_event,
                                    ),
                                ),
                            };
                            ws_bus.broadcast(&topic, msg).await;

                            let sys_topic = WsTopic {
                                kind: WsTopicKind::System,
                                id: TopicId(0),
                            };
                            ws_bus
                                .broadcast(
                                    &sys_topic,
                                    ServerMessage::Event {
                                        topic: sys_topic.clone(),
                                        event_type: "agent_status".to_string(),
                                        timestamp: chrono::Utc::now(),
                                        payload: serde_json::json!({"action": "refresh"}),
                                        html: None,
                                    },
                                )
                                .await;
                        }
                    });

                    Ok(StatusCode::OK)
                }
                Err(e) => {
                    tracing::warn!(
                        task_id = %task_id,
                        conversation_id = %conv_id,
                        session_id = %provider_session_id,
                        error = %e,
                        "Failed to resume turn, removing dead provider"
                    );
                    drop(sessions);
                    Box::pin(send_message(
                        auth,
                        State(state),
                        Path((task_id, conv_id)),
                        Json(SendMessageRequest {
                            text: body.text.clone(),
                        }),
                    ))
                    .await
                }
            }
        }
        None => {
            tracing::warn!(
                task_id = %task_id,
                conversation_id = %conv_id,
                "No active provider — attempting lazy recreation after restart"
            );

            let psid = conv.provider_session_id.as_deref().unwrap_or("");
            if psid.starts_with("UNSET_") {
                return Err(ServerError::NotFound(
                    "Session was never started. Start a new agent run.".into(),
                ));
            }

            drop(sessions);

            let run = tasks::get_agent_run_by_conversation(&state.db, &conv_id)
                .await
                .map_err(|_| {
                    ServerError::NotFound("No active session for this conversation".into())
                })?;

            let agent_type = AgentType::from_str(&run.agent_type.to_string())
                .map_err(|_| ServerError::Internal("Invalid agent type".into()))?;

            let harness_config = registry::resolve_harness_config(
                &state.db,
                &agent_type,
                Some(&task.user_id),
                Some(task.project_id),
            )
            .await
            .map_err(|e| {
                ServerError::Internal(format!("Failed to resolve provider config: {e}"))
            })?;

            let config_root = PathBuf::from(&state.config_root);
            let mut provider = registry::resolve_provider_for_user(
                &harness_config,
                &config_root,
                task.user_id,
                state.config.info_log_client_data,
            )
            .await
            .map_err(|e| ServerError::Internal(format!("Failed to resolve provider: {e}")))?;

            let worktree = tasks::get_worktree_by_task(&state.db, task_id).await.ok();
            let working_dir = worktree
                .as_ref()
                .map(|w| PathBuf::from(&w.worktree_path))
                .unwrap_or_else(|| PathBuf::from("/tmp"));

            provider
                .start(&working_dir)
                .await
                .map_err(|e| ServerError::Internal(format!("Failed to start provider: {e}")))?;

            state
                .active_sessions
                .lock()
                .await
                .insert(conv_id.to_string(), provider);

            // Retry — the provider is now in active_sessions
            Box::pin(send_message(
                auth,
                State(state),
                Path((task_id, conv_id)),
                Json(SendMessageRequest {
                    text: body.text.clone(),
                }),
            ))
            .await
        }
    }
}
