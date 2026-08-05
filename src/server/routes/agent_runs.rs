use std::path::PathBuf;
use std::str::FromStr;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::post,
    Json, Router,
};
use serde::Deserialize;

use crate::auth::AuthUser;
use crate::db::schema::{AgentType, TaskAgentRun};
use crate::orchestration;
use crate::providers::types::ProviderEvent;
use crate::server::ws::message::{ServerMessage, TopicId, WsTopic, WsTopicKind};
use crate::server::{error::ServerError, state::AppState};
use crate::services::tasks;

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

    let config_root = PathBuf::from(&state.config_root);

    let run = crate::orchestration::start_next_agent(
        &state.db,
        &task,
        agent_type,
        &config_root,
        &state.footprint,
        &state.archive_root,
        &state.active_sessions,
        &state.ws_bus,
        &state.config,
    )
    .await?;

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
        .filter_map(|mut row| -> Option<String> { row.get("conversation_id") })
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
    let topic = WsTopic {
        kind: WsTopicKind::Task,
        id: TopicId(task_id),
    };
    let error_event = ProviderEvent::Error {
        error: "Session reset — you can now start a new agent run.".into(),
        timestamp: chrono::Utc::now().naive_utc(),
    };
    let msg = ServerMessage::Event {
        topic: topic.clone(),
        event_type: "error".to_string(),
        timestamp: chrono::Utc::now(),
        payload: serde_json::json!({"error": "Session reset — you can now start a new agent run."}),
        html: Some(crate::webapp::components::message_stream::render_event(
            &error_event,
        )),
    };
    state.ws_bus.broadcast(&topic, msg).await;

    crate::orchestration::broadcast_agent_status(&state.ws_bus, "stopped").await;

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
