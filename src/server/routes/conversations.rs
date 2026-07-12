use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::db::schema::{Conversation, ConversationWithRun, TaskAgentRun};
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
        .route("/{id}/abort", post(abort_turn))
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

    let omp_session_id = conv.omp_session_id.clone().unwrap_or_default();
    let messages = transcript::load_transcript(&state.db, &omp_session_id, conv.task_id)
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
    tracing::info!(
        task_id = %task_id,
        conversation_id = %conv_id,
        "Sending message to resume session"
    );

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

    if body.text.trim().is_empty() {
        return Err(ServerError::BadRequest("message text is required".into()));
    }

    // Persist the user's message
    let user_event = ProviderEvent::UserText {
        text: body.text.clone(),
    };
    let omp_session_id = conv.omp_session_id.clone().unwrap_or_default();
    transcript::persist_event(&state.db, &user_event, &omp_session_id, task_id)
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
            tracing::debug!(
                task_id = %task_id,
                conversation_id = %conv_id,
                session_id = %omp_session_id,
                "Found active provider, loading transcript"
            );

            let messages = transcript::load_transcript(&state.db, &omp_session_id, task_id)
                .await
                .map_err(|e| ServerError::Internal(e.to_string()))?;

            tracing::debug!(
                task_id = %task_id,
                conversation_id = %conv_id,
                message_count = messages.len(),
                "Loaded transcript"
            );

            let messages_json = serde_json::to_value(&messages)
                .map_err(|e| ServerError::Internal(e.to_string()))?;

            let resume_input = ResumeInput::new(omp_session_id.clone(), messages_json);

            match p.resume_turn(resume_input).await {
                Ok(mut rx) => {
                    tracing::info!(
                        task_id = %task_id,
                        conversation_id = %conv_id,
                        session_id = %omp_session_id,
                        "Successfully resumed turn, spawning broadcast task"
                    );
                    let db = state.db.clone();
                    let ws_bus = state.ws_bus.clone();
                    let active_sessions = state.active_sessions.clone();
                    let t_id = task_id;
                    let s_id = omp_session_id;
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

                                    let topic = WsTopic {
                                        kind: WsTopicKind::Task,
                                        id: TopicId(t_id),
                                    };

                                    let (event_type, payload) = match &event {
                                        ProviderEvent::SessionStart { session_id } => {
                                            ("session_start".to_string(), serde_json::json!({"session_id": session_id}))
                                        }
                                        ProviderEvent::UserText { text } => {
                                            ("user_text".to_string(), serde_json::json!({"text": text}))
                                        }
                                        ProviderEvent::Text { text } => {
                                            ("text".to_string(), serde_json::json!({"text": text}))
                                        }
                                        ProviderEvent::TextChunk { delta } => {
                                            ("text_chunk".to_string(), serde_json::json!({"delta": delta}))
                                        }
                                        ProviderEvent::ToolUse { tool_name, tool_use_id, input } => {
                                            ("tool_use".to_string(), serde_json::json!({
                                                "tool_name": tool_name,
                                                "tool_use_id": tool_use_id,
                                                "input": input,
                                            }))
                                        }
                                        ProviderEvent::ToolResult { tool_use_id, result } => {
                                            ("tool_result".to_string(), serde_json::json!({
                                                "tool_use_id": tool_use_id,
                                                "result": result,
                                            }))
                                        }
                                        ProviderEvent::Thinking { thinking } => {
                                            ("thinking".to_string(), serde_json::json!({"thinking": thinking}))
                                        }
                                        ProviderEvent::ThinkingChunk { delta } => {
                                            ("thinking_chunk".to_string(), serde_json::json!({"delta": delta}))
                                        }
                                        ProviderEvent::ContextUsage(usage) => {
                                            ("context_usage".to_string(), serde_json::json!({"usage": usage}))
                                        }
                                        ProviderEvent::ExtensionUiRequest(data) => {
                                            ("extension_ui_request".to_string(), data.clone())
                                        }
                                        ProviderEvent::AvailableCommandsUpdate(data) => {
                                            ("available_commands_update".to_string(), data.clone())
                                        }
                                        ProviderEvent::Response(data) => {
                                            ("response".to_string(), data.clone())
                                        }
                                        ProviderEvent::Error { error } => {
                                            ("error".to_string(), serde_json::json!({"error": error}))
                                        }
                                        ProviderEvent::Done(data) => {
                                            completed_normally = true;
                                            ("done".to_string(), serde_json::json!({"data": data}))
                                        }
                                        ProviderEvent::Ready => {
                                            ("ready".to_string(), serde_json::json!({}))
                                        }
                                    };

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
                }
                Err(e) => {
                    tracing::warn!(
                        task_id = %task_id,
                        conversation_id = %conv_id,
                        session_id = %omp_session_id,
                        error = %e,
                        "Failed to resume turn"
                    );
                }
            }

            Ok(StatusCode::OK)
        }
        None => {
            tracing::warn!(
                task_id = %task_id,
                conversation_id = %conv_id,
                "No active provider session found for conversation"
            );
            Err(ServerError::NotFound(
                "No active provider session for this conversation".into(),
            ))
        }
    }
}

async fn abort_turn(
    auth: AuthUser,
    State(state): State<AppState>,
    Path((task_id, conv_id)): Path<(i64, Uuid)>,
) -> Result<StatusCode, ServerError> {
    tracing::info!(
        task_id = %task_id,
        conversation_id = %conv_id,
        "Aborting current turn"
    );

    let task = tasks::get_task(&state.db, task_id)
        .await
        .map_err(|_| ServerError::NotFound("Task not found".into()))?;
    if task.user_id != auth.user_id {
        return Err(ServerError::NotFound("Task not found".into()));
    }

    let sessions = state.active_sessions.lock().await;
    let provider = sessions
        .get(&conv_id.to_string())
        .ok_or_else(|| {
            tracing::warn!(task_id = %task_id, conversation_id = %conv_id, "No active session to abort");
            ServerError::NotFound("No active session".into())
        })?;

    provider
        .abort_turn()
        .await
        .map_err(|e| {
            tracing::warn!(task_id = %task_id, conversation_id = %conv_id, error = %e, "Failed to abort turn");
            ServerError::Internal(e.to_string())
        })?;

    tracing::info!(
        task_id = %task_id,
        conversation_id = %conv_id,
        "Turn aborted successfully"
    );

    Ok(StatusCode::OK)
}
