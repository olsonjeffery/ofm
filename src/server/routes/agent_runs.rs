use std::path::PathBuf;
use std::str::FromStr;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::post,
    Json, Router,
};
use serde::Deserialize;

use crate::agents::{self, pull_request::PullRequestStatus};
use crate::auth::AuthUser;
use crate::db::schema::{AgentType, TaskAgentRun};
use crate::orchestration;
use crate::orchestration::guards;
use crate::providers::registry;
use crate::providers::types::{ProviderEvent, TurnInput};
use crate::server::ws::message::{ServerMessage, TopicId, WsTopic, WsTopicKind};
use crate::server::{error::ServerError, state::AppState};
use crate::services::session;
use crate::services::tasks;
use futures_util::FutureExt;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Debug, Deserialize)]
struct StartAgentRunRequest {
    agent_type: String,
}

pub fn agent_runs_router() -> Router<AppState> {
    Router::new()
        .route("/", post(post_create_agent_run).get(list_agent_runs))
        .route("/reset", post(reset_agent_runs))
        .route("/stop", post(reset_agent_runs))
}

async fn post_create_agent_run(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(task_id): Path<i64>,
    Json(body): Json<StartAgentRunRequest>,
) -> Result<(StatusCode, Json<TaskAgentRun>), ServerError> {
    let agent_type = AgentType::from_str(&body.agent_type).map_err(ServerError::BadRequest)?;

    tracing::info!(
        task_id = %task_id,
        agent_type = %body.agent_type,
        "Starting agent run"
    );

    let task = tasks::get_task(&state.db, task_id)
        .await
        .map_err(|_| ServerError::NotFound("Task not found".into()))?;
    if task.user_id != auth.user_id {
        return Err(ServerError::NotFound("Task not found".into()));
    }

    // Phase 3: Config check guard — if no config, create blocked run and skip
    let config_root = PathBuf::from(&state.config_root);
    let harness_config = match registry::resolve_harness_config(
        &state.db,
        &agent_type,
        Some(&task.user_id),
        Some(task.project_id),
    )
    .await
    {
        Ok(cfg) => {
            tracing::debug!(
                task_id = %task_id,
                agent_type = %body.agent_type,
                model = ?cfg.model,
                effort = ?cfg.effort,
                "Resolved provider config"
            );
            cfg
        }
        Err(e) => {
            tracing::warn!(
                task_id = %task_id,
                agent_type = %body.agent_type,
                error = %e,
                "No provider config found, creating blocked run"
            );
            let run = tasks::create_agent_run_blocked(&state.db, task_id, &agent_type)
                .await
                .map_err(|e| ServerError::Internal(e.to_string()))?;
            return Ok((StatusCode::CREATED, Json(run)));
        }
    };

    guards::one_running_per_task(&state.db, task_id).await?;

    guards::iteration_cap(&task)?;

    tasks::increment_workflow_run_count(&state.db, task_id)
        .await
        .map_err(orchestration::internal_err)?;

    let model = harness_config
        .model
        .as_deref()
        .unwrap_or("default")
        .to_string();
    let effort = harness_config
        .effort
        .as_deref()
        .unwrap_or("balanced")
        .to_string();

    let session_result =
        session::start_session(&state.db, task_id, &model, &effort, agent_type.clone())
            .await
            .map_err(|e| match &e {
                hiqlite::Error::ConstraintViolation(_) => {
                    tracing::warn!(task_id = %task_id, agent_type = %body.agent_type, "Agent already running for this task");
                    ServerError::Conflict("an agent is already running for this task".into())
                }
                _ => ServerError::Internal(e.to_string()),
            })?;

    tracing::info!(
        task_id = %task_id,
        agent_type = %body.agent_type,
        conversation_id = %session_result.conversation_id,
        session_id = %session_result.session_id,
        "Session created successfully"
    );

    // Start and store provider, then begin turn
    match registry::resolve_provider_for_user(&harness_config, &config_root, task.user_id).await {
        Ok(mut provider) => {
            let working_dir = std::path::Path::new("/tmp");
            match provider.start(working_dir).await {
                Ok(()) => {
                    let conv_id_str = session_result.conversation_id.to_string();

                    // Build TurnInput from task doc + context prompt
                    let archive =
                        crate::archive::ArchiveRoot::new(PathBuf::from(&state.archive_root));
                    let worktree = tasks::get_worktree_by_task(&state.db, task_id).await.ok();
                    let cwd = worktree
                        .as_ref()
                        .map(|w| w.worktree_path.clone())
                        .unwrap_or_else(|| "/tmp".to_string());

                    let proj_str = task.project_id.to_string();
                    let task_str = task_id.to_string();
                    let doc_path = archive.task_doc_path(&proj_str, &task_str);
                    let context_prompt = archive
                        .build_context_prompt(&state.footprint, task.project_id, task_id)
                        .ok()
                        .unwrap_or_default();

                    let prompt_text = {
                        let phase_prompt = match agent_type {
                            AgentType::Planification => agents::planning::build_planning_prompt(
                                "",
                                &doc_path.to_string_lossy(),
                                &task_str,
                                "",
                            ),
                            AgentType::Implementation => {
                                agents::implementation::build_implementation_prompt(
                                    &doc_path.to_string_lossy(),
                                )
                            }
                            AgentType::Review => agents::review::build_review_prompt(
                                task_id,
                                &doc_path.to_string_lossy(),
                            ),
                            AgentType::Refinement => agents::refinement::build_refinement_prompt(
                                task_id,
                                &doc_path.to_string_lossy(),
                            ),
                            // FIXME: need to thread in real PR status in case it does in fact
                            // exist..
                            AgentType::Pr => agents::pull_request::build_pull_request_prompt(
                                task_id,
                                &doc_path.to_string_lossy(),
                                &PullRequestStatus::NoPr,
                            ),
                            _ => String::new(),
                        };
                        if context_prompt.is_empty() {
                            phase_prompt
                        } else if phase_prompt.is_empty() {
                            context_prompt
                        } else {
                            format!("{}\n\n{}", phase_prompt, context_prompt)
                        }
                    };

                    let turn_input = TurnInput::new(
                        prompt_text.clone(),
                        cwd,
                        model,
                        effort,
                        "auto".to_string(),
                        vec![],
                        String::new(),
                    )
                    .session_id(session_result.session_id.clone());

                    // Broadcast initial prompt as user message before start_turn
                    let prompt_event = ProviderEvent::UserText {
                        text: prompt_text.clone(),
                    };
                    if let Err(e) = crate::services::transcript::persist_event(
                        &state.db,
                        &prompt_event,
                        &session_result.session_id,
                        task_id,
                    )
                    .await
                    {
                        tracing::warn!("Failed to persist prompt event: {e}");
                    }
                    let topic = WsTopic {
                        kind: WsTopicKind::Task,
                        id: TopicId(task_id),
                    };
                    let msg = ServerMessage::Event {
                        topic: topic.clone(),
                        event_type: "user_text".to_string(),
                        timestamp: chrono::Utc::now(),
                        payload: serde_json::json!({"text": prompt_text}),
                    };
                    state.ws_bus.broadcast(&topic, msg).await;

                    match provider.start_turn(turn_input).await {
                        Ok(mut rx) => {
                            // Store provider before spawning task (rx is independent)
                            state
                                .active_sessions
                                .lock()
                                .await
                                .insert(conv_id_str, provider);

                            // Spawn broadcast task
                            let db = state.db.clone();
                            let ws_bus = state.ws_bus.clone();
                            let active_sessions = state.active_sessions.clone();
                            let conversation_id = session_result.conversation_id;
                            let t_id = task_id;
                            let project_key = task_id;
                            let mut s_id = session_result.session_id;

                            tokio::spawn(async move {
                                let completed_normally = Arc::new(AtomicBool::new(false));
                                let cn = completed_normally.clone();
                                let db_inner = db.clone();
                                let ws_bus_inner = ws_bus.clone();
                                let active_sessions_inner = active_sessions.clone();
                                let active_sessions_for_guard = active_sessions_inner.clone();

                                let broadcast_fut = AssertUnwindSafe(async move {
                                    let mut local_completed = false;
                                    loop {
                                        tokio::select! {
                                            event = rx.recv() => {
                                                let event = match event {
                                                    Some(e) => e,
                                                    None => break,
                                                };

                                                if let Err(e) = crate::services::transcript::persist_event(
                                                    &db_inner, &event, &s_id, project_key
                                                ).await {
                                                    tracing::warn!("Failed to persist event: {e}");
                                                }

                                                if let ProviderEvent::SessionStart { session_id } = &event {
                                                    s_id = session_id.clone();
                                                    let _ = db_inner.execute(
                                                        "UPDATE conversations SET provider_session_id = $1 WHERE id = $2",
                                                        hiqlite::params!(session_id, conversation_id.to_string()),
                                                    ).await;
                                                }

                                                let topic = WsTopic {
                                                    kind: WsTopicKind::Task,
                                                    id: TopicId(t_id),
                                                };

                                                let (event_type, payload) = event.to_ws_event();
                                                if matches!(event, ProviderEvent::Done(_)) {
                                                    local_completed = true;
                                                }

                                                let msg = ServerMessage::Event {
                                                    topic: topic.clone(),
                                                    event_type,
                                                    timestamp: chrono::Utc::now(),
                                                    payload,
                                                };

                                                ws_bus_inner.broadcast(&topic, msg).await;

                                                if matches!(event, ProviderEvent::Done(_)) {
                                                    if let Err(e) = crate::orchestration::completion_handler(
                                                        &db_inner, conversation_id, &active_sessions_inner
                                                    ).await {
                                                        tracing::warn!("Error in completion handler: {e:?}");
                                                    }
                                                    break;
                                                }
                                            }
                                        }
                                    }
                                    if local_completed {
                                        cn.store(true, Ordering::SeqCst);
                                    }
                                });

                                let result = broadcast_fut.catch_unwind().await;
                                if let Err(ref panic) = result {
                                    tracing::error!("Broadcast task panicked: {panic:?}");
                                }

                                if !completed_normally.load(Ordering::SeqCst) {
                                    // Abort the in-flight turn without killing the
                                    // pooled opencode server. The provider stays in
                                    // `active_sessions` so the user can resume
                                    // the conversation; the underlying server is
                                    // reaped by the idle-reaper or process-exit
                                    // cleanup in `src/main.rs`.
                                    {
                                        let sessions = active_sessions_for_guard.lock().await;
                                        if let Some(p) = sessions.get(&conversation_id.to_string())
                                        {
                                            if let Err(e) = p.abort_turn().await {
                                                tracing::warn!(conversation_id = %conversation_id, "Error aborting provider in broadcast cleanup: {e}");
                                            }
                                        }
                                    }

                                    // Mark run as failed and notify UI
                                    let _ = db.execute(
                                        "UPDATE task_agent_runs SET status = 'failed' WHERE conversation_id = $1",
                                        hiqlite::params!(conversation_id.to_string()),
                                    ).await;
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
                            tracing::warn!("Failed to start turn: {e}");
                            state
                                .active_sessions
                                .lock()
                                .await
                                .insert(conv_id_str, provider);
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to start provider: {e}");
                }
            }
        }
        Err(e) => {
            tracing::warn!("Failed to resolve provider: {e}");
        }
    }

    // NOTE: Fire-and-forget title generation via a transient opencode
    // subprocess is disabled temporarily. The transient server it spawned
    // was escaping Stop Agent's cleanup (it isn't tracked in
    // active_sessions) and leaving a second live opencode process behind
    // after Stop Agent was pressed. It also produced inconsistent titles.
    // Re-enable only after reworking it to register with active_sessions
    // or run on the in-flight provider's existing server.
    //
    // let db = state.db.clone();
    // let cfg_root = PathBuf::from(&state.config_root);
    // let title_config = harness_config.clone();
    // let conv_id = session_result.conversation_id;
    // let task_title = task.title.clone();
    // let first_message = format!("{} phase, task title: {}", agent_type, task_title);
    // tokio::spawn(async move {
    //     providers::generate_conversation_title(
    //         &db,
    //         &cfg_root,
    //         &title_config,
    //         conv_id,
    //         &first_message,
    //     )
    //     .await;
    // });
    let run = tasks::get_agent_run_by_conversation(&state.db, &session_result.conversation_id)
        .await
        .map_err(orchestration::internal_err)?;

    Ok((StatusCode::CREATED, Json(run)))
}

async fn reset_agent_runs(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(task_id): Path<i64>,
) -> Result<StatusCode, ServerError> {
    let task = tasks::get_task(&state.db, task_id)
        .await
        .map_err(|_| ServerError::NotFound("Task not found".into()))?;
    if task.user_id != auth.user_id {
        return Err(ServerError::NotFound("Task not found".into()));
    }

    tracing::info!(task_id = %task_id, "Resetting agent runs for task");

    // Get conversation IDs for this task's agent runs — include ALL
    // conversations (not just running ones) so that lazily-recreated
    // providers (which may not have a corresponding running agent run)
    // are also caught and shut down.
    let conv_ids: Vec<String> = state
        .db
        .query_raw(
            "SELECT DISTINCT conversation_id FROM task_agent_runs WHERE task_id = $1 AND conversation_id IS NOT NULL",
            hiqlite::params!(task_id),
        )
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?
        .into_iter()
        .filter_map(|mut row| {
            let conv_id: Option<String> = row.get("conversation_id");
            conv_id
        })
        .collect();

    tracing::info!(
        task_id = %task_id,
        conversation_count = conv_ids.len(),
        "Found active sessions to reset"
    );

    // Abort any in-flight turn on matching providers WITHOUT shutting down
    // the underlying opencode server. This mirrors the reference
    // implementation's `abortTurn` (see
    // `spec/reference/server/services/providers/opencode/index.ts`): the
    // server is persistent across Stop Agent / turn completion so the
    // session_id stored in the DB remains valid for a subsequent resume.
    // The server is only killed when the ofm process exits (see the
    // signal handlers in `src/main.rs`).
    //
    // abort_turn is fast: it flips a cancellation flag and fires a
    // best-effort HTTP POST. The lock is held briefly for the abort
    // sequence only.
    {
        let sessions = state.active_sessions.lock().await;
        for conv_id in &conv_ids {
            if let Some(provider) = sessions.get(conv_id) {
                tracing::debug!(
                    task_id = %task_id,
                    conversation_id = %conv_id,
                    "Aborting in-flight turn for Stop Agent"
                );
                if let Err(e) = provider.abort_turn().await {
                    tracing::warn!(
                        task_id = %task_id,
                        conversation_id = %conv_id,
                        error = %e,
                        "Error aborting turn during reset"
                    );
                }
            }
        }
    }

    // Mark all running runs for this task as failed
    let affected = state
        .db
        .execute(
            "UPDATE task_agent_runs SET status = 'failed', completed_at = $2 WHERE task_id = $1 AND status = 'running'",
            hiqlite::params!(task_id, chrono::Utc::now().naive_utc().to_string()),
        )
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;

    tracing::info!(task_id = %task_id, affected_runs = affected, "Marked running runs as failed");

    // Broadcast reset notification
    let topic = crate::server::ws::message::WsTopic {
        kind: crate::server::ws::message::WsTopicKind::Task,
        id: crate::server::ws::message::TopicId(task_id),
    };
    let msg = crate::server::ws::message::ServerMessage::Event {
        topic: topic.clone(),
        event_type: "error".to_string(),
        timestamp: chrono::Utc::now(),
        payload: serde_json::json!({"error": "Session reset — you can now start a new agent run."}),
    };
    state.ws_bus.broadcast(&topic, msg).await;

    Ok(StatusCode::OK)
}

async fn list_agent_runs(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(task_id): Path<i64>,
) -> Result<Json<Vec<TaskAgentRun>>, ServerError> {
    let task = tasks::get_task(&state.db, task_id)
        .await
        .map_err(|_| ServerError::NotFound("Task not found".into()))?;
    if task.user_id != auth.user_id {
        return Err(ServerError::NotFound("Task not found".into()));
    }

    let runs = tasks::list_agent_runs_for_task(&state.db, task_id)
        .await
        .map_err(orchestration::internal_err)?;

    Ok(Json(runs))
}
