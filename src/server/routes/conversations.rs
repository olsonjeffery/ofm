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
use crate::server::ws::message::{ServerMessage, TopicId, WsTopic, WsTopicKind};
use crate::server::{error::ServerError, state::AppState};
use crate::services::{session, tasks, transcript};

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
        .route("/{id}/abort", post(stop_agent))
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

    if body.text.trim().is_empty() {
        return Err(ServerError::BadRequest("message text is required".into()));
    }

    // Persist the user's message
    let user_event = ProviderEvent::UserText {
        text: body.text.clone(),
    };
    transcript::persist_event(&state.db, &user_event, &provider_session_id, task_id)
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;

    // Broadcast user message via WS
    let topic = WsTopic {
        kind: WsTopicKind::Task,
        id: TopicId(task_id),
    };
    let msg = ServerMessage::Event {
        topic: topic.clone(),
        event_type: "user_text".to_string(),
        timestamp: chrono::Utc::now(),
        payload: serde_json::json!({"text": body.text}),
    };
    state.ws_bus.broadcast(&topic, msg).await;

    // Load transcript and resume the provider
    let sessions = state.active_sessions.lock().await;
    let provider = sessions.get(&conv_id.to_string());

    match provider {
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

            let mut rx = p.resume_turn(resume_input).await.map_err(|e| {
                tracing::warn!(
                    task_id = %task_id,
                    conversation_id = %conv_id,
                    session_id = %provider_session_id,
                    error = %e,
                    "Failed to resume turn"
                );
                ServerError::Internal(format!("Failed to resume turn: {e}"))
            })?;

            tracing::info!(
                task_id = %task_id,
                conversation_id = %conv_id,
                session_id = %provider_session_id,
                "Successfully resumed turn, spawning broadcast task"
            );
            let db = state.db.clone();
            let ws_bus = state.ws_bus.clone();
            let active_sessions = state.active_sessions.clone();
            let t_id = task_id;
            let s_id = provider_session_id;
            let project_key = task_id;
            let c_id = conv_id;

            tokio::spawn(async move {
                let mut completed_normally = false;
                loop {
                    tokio::select! {
                        event = rx.recv() => {
                            let event = match event {
                                Some(e) => e,
                                None => break,
                            };

                            if let Err(e) = transcript::persist_event(
                                &db, &event, &s_id, project_key
                            ).await {
                                tracing::warn!("Failed to persist event: {e}");
                            }

                            if let ProviderEvent::SessionStart { session_id } = &event {
                                let _ = db.execute(
                                    "UPDATE conversations SET provider_session_id = $1 WHERE id = $2",
                                    hiqlite::params!(session_id, c_id.to_string()),
                                ).await;
                            }

                            let topic = WsTopic {
                                kind: WsTopicKind::Task,
                                id: TopicId(t_id),
                            };

                            let (event_type, payload) = event.to_ws_event();
                            if matches!(event, ProviderEvent::Done(_)) {
                                completed_normally = true;
                            }

                            let msg = ServerMessage::Event {
                                topic: topic.clone(),
                                event_type,
                                timestamp: chrono::Utc::now(),
                                payload,
                            };

                            ws_bus.broadcast(&topic, msg).await;

                            if matches!(event, ProviderEvent::Done(_)) {
                                if let Err(e) = crate::orchestration::completion_handler(
                                    &db, c_id, &active_sessions
                                ).await {
                                    tracing::warn!("Error in completion handler: {e:?}");
                                }
                                break;
                            }
                        }
                    }
                }
                if !completed_normally {
                    // Remove provider from active_sessions and shut down
                    if let Some(mut provider) =
                        active_sessions.lock().await.remove(&c_id.to_string())
                    {
                        if let Err(e) = provider.shutdown().await {
                            tracing::warn!("Error shutting down provider after abnormal end: {e}");
                        }
                    }

                    let topic = WsTopic {
                        kind: WsTopicKind::Task,
                        id: TopicId(t_id),
                    };
                    let msg = ServerMessage::Event {
                        topic: topic.clone(),
                        event_type: "error".to_string(),
                        timestamp: chrono::Utc::now(),
                        payload: serde_json::json!({"error": "Agent session ended unexpectedly. Send a message to resume."}),
                    };
                    ws_bus.broadcast(&topic, msg).await;
                }
            });

            Ok(StatusCode::OK)
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
            let mut provider = registry::resolve_provider(
                &harness_config,
                std::path::Path::new("omp"),
                &config_root,
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

async fn stop_agent(
    auth: AuthUser,
    State(state): State<AppState>,
    Path((task_id, conv_id)): Path<(i64, Uuid)>,
) -> Result<StatusCode, ServerError> {
    tracing::info!(
        task_id = %task_id,
        conversation_id = %conv_id,
        "Stopping agent"
    );

    let task = tasks::get_task(&state.db, task_id)
        .await
        .map_err(|_| ServerError::NotFound("Task not found".into()))?;
    if task.user_id != auth.user_id {
        return Err(ServerError::NotFound("Task not found".into()));
    }

    let mut sessions = state.active_sessions.lock().await;
    if let Some(mut provider) = sessions.remove(&conv_id.to_string()) {
        let _ = provider.abort_turn().await; // best-effort
        if let Err(e) = provider.shutdown().await {
            tracing::warn!("Error shutting down provider: {e}");
        }
    }
    drop(sessions);

    let _ = state.db.execute(
        "UPDATE task_agent_runs SET status = 'failed', completed_at = $2 WHERE conversation_id = $1 AND status = 'running'",
        hiqlite::params!(conv_id.to_string(), chrono::Utc::now().naive_utc().to_string()),
    ).await;

    let topic = WsTopic {
        kind: WsTopicKind::Task,
        id: TopicId(task_id),
    };
    let msg = ServerMessage::Event {
        topic: topic.clone(),
        event_type: "error".to_string(),
        timestamp: chrono::Utc::now(),
        payload: serde_json::json!({"error": "Agent session ended unexpectedly. Send a message to resume."}),
    };
    state.ws_bus.broadcast(&topic, msg).await;

    Ok(StatusCode::OK)
}
