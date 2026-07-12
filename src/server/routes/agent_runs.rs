use std::path::PathBuf;
use std::str::FromStr;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::post,
    Json, Router,
};
use serde::Deserialize;

use crate::agents;
use crate::auth::AuthUser;
use crate::db::schema::{AgentType, TaskAgentRun};
use crate::orchestration;
use crate::orchestration::guards;
use crate::providers;
use crate::providers::registry;
use crate::providers::types::{ProviderEvent, TurnInput};
use crate::server::ws::message::{ServerMessage, WsTopic};
use crate::server::{error::ServerError, state::AppState};
use crate::services::session;
use crate::services::tasks;

fn strip_agent_directive(s: &str) -> String {
    s.lines()
        .skip_while(|line| line.starts_with("@agent-"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Debug, Deserialize)]
struct StartAgentRunRequest {
    agent_type: String,
}

pub fn agent_runs_router() -> Router<AppState> {
    Router::new()
        .route("/", post(post_create_agent_run).get(list_agent_runs))
        .route("/reset", post(reset_agent_runs))
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
    match registry::resolve_provider(&harness_config, std::path::Path::new("omp"), &config_root)
        .await
    {
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
                        .build_context_prompt(&proj_str, &task_str)
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
                                agents::implementation::build_implementation_prompt("")
                            }
                            AgentType::Review => agents::review::build_review_prompt(""),
                            AgentType::Pr => agents::pull_request::build_pull_request_prompt(
                                "",
                                &agents::pull_request::PullRequestStatus::NoPr,
                            ),
                            AgentType::Refinement => {
                                agents::refinement::build_refinement_prompt("")
                            }
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
                    let prompt_text = strip_agent_directive(&prompt_text);

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
                    let topic = WsTopic::task(task_id);
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
                            let s_id = session_result.session_id;
                            let project_key = task_id;

                            tokio::spawn(async move {
                                let mut completed_normally = false;
                                loop {
                                    tokio::select! {
                                                        event = rx.recv() => {
                                                            let event = match event {
                                                                Some(e) => e,
                                                                None => break,
                                                            };

                                                            // Persist event
                                                            if let Err(e) = crate::services::transcript::persist_event(
                                                                &db, &event, &s_id, project_key
                                                            ).await {
                                                                tracing::warn!("Failed to persist event: {e}");
                                                            }

                                    let topic = WsTopic::task(t_id);

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
                                                                    &db, conversation_id, &active_sessions
                                                                ).await {
                                                                    tracing::warn!("Error in completion handler: {e:?}");
                                                                }
                                                                break;
                                                            }
                                                        }
                                                    }
                                }
                                if !completed_normally {
                                    // Mark run as failed and notify UI
                                    let _ = db.execute(
                                        "UPDATE task_agent_runs SET status = 'failed' WHERE conversation_id = $1",
                                        hiqlite::params!(conversation_id.to_string()),
                                    ).await;
                                    let topic = WsTopic::task(t_id);
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

    // Fire-and-forget title generation
    let db = state.db.clone();
    let cfg_root = PathBuf::from(&state.config_root);
    let title_config = harness_config.clone();
    let conv_id = session_result.conversation_id;
    let task_title = task.title.clone();
    tokio::spawn(async move {
        providers::generate_conversation_title(&db, &cfg_root, &title_config, conv_id, &task_title)
            .await;
    });

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

    // Get conversation IDs for this task's agent runs
    let conv_ids: Vec<String> = state
        .db
        .query_raw(
            "SELECT conversation_id FROM task_agent_runs WHERE task_id = $1 AND status = 'running' AND conversation_id IS NOT NULL",
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

    // Only clear sessions for this task's conversations
    let mut providers_to_shutdown = Vec::new();
    {
        let mut sessions = state.active_sessions.lock().await;
        for conv_id in &conv_ids {
            if let Some(provider) = sessions.remove(conv_id) {
                providers_to_shutdown.push(provider);
                tracing::debug!(task_id = %task_id, conversation_id = %conv_id, "Removed session from active_sessions");
            }
        }
    }

    // Shutdown the removed providers
    for mut p in providers_to_shutdown {
        if let Err(e) = p.shutdown().await {
            tracing::warn!(task_id = %task_id, error = %e, "Error shutting down provider during reset");
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
    let topic = WsTopic::task(task_id);
    let msg = ServerMessage::Event {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_agent_directive_none() {
        let input = "Hello world\nThis is a test";
        assert_eq!(strip_agent_directive(input), input);
    }

    #[test]
    fn test_strip_agent_directive_first_line() {
        let input = "@agent-Plan Do planning stuff\n## Primary Goal\nDo the thing";
        let expected = "## Primary Goal\nDo the thing";
        assert_eq!(strip_agent_directive(input), expected);
    }

    #[test]
    fn test_strip_agent_directive_only_directive() {
        let input = "@agent-Implement";
        assert_eq!(strip_agent_directive(input), "");
    }

    #[test]
    fn test_strip_agent_directive_multiple_lines() {
        let input = "@agent-Review\n\n## Section 1\ncontent\n## Section 2\nmore content";
        let expected = "\n## Section 1\ncontent\n## Section 2\nmore content";
        assert_eq!(strip_agent_directive(input), expected);
    }

    #[test]
    fn test_strip_agent_directive_no_trailing_newline() {
        let input = "@agent-PR\ncontent";
        assert_eq!(strip_agent_directive(input), "content");
    }
}
