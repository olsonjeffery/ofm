use std::str::FromStr;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::post,
    Json, Router,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::db::schema::{AgentType, TaskAgentRun};
use crate::omp::session;
use crate::orchestration::guards;
use crate::server::error::ServerError;
use crate::server::state::AppState;
use crate::services::tasks;

#[derive(Debug, Deserialize)]
struct StartAgentRunRequest {
    agent_type: String,
}

pub fn agent_runs_router() -> Router<AppState> {
    Router::new().route("/", post(post_create_agent_run).get(list_agent_runs))
}

async fn post_create_agent_run(
    State(state): State<AppState>,
    Path(task_id): Path<Uuid>,
    Json(body): Json<StartAgentRunRequest>,
) -> Result<(StatusCode, Json<TaskAgentRun>), ServerError> {
    let agent_type = AgentType::from_str(&body.agent_type).map_err(ServerError::BadRequest)?;

    let task = tasks::get_task(&state.db, &task_id)
        .await
        .map_err(|_| ServerError::NotFound("Task not found".into()))?;

    guards::one_running_per_task(&state.db, task_id).await?;

    // TODO: check user credentials when credential system is built (→ 403 Forbidden)

    guards::iteration_cap(&task)?;

    tasks::increment_workflow_run_count(&state.db, &task_id)
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;

    let session_result =
        session::start_session(&state.db, task_id, "default", "balanced", agent_type)
            .await
            .map_err(|e| match &e {
                hiqlite::Error::ConstraintViolation(_) => {
                    ServerError::Conflict("an agent is already running for this task".into())
                }
                _ => ServerError::Internal(e.to_string()),
            })?;

    let run = tasks::get_agent_run_by_conversation(&state.db, &session_result.conversation_id)
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;

    Ok((StatusCode::CREATED, Json(run)))
}

async fn list_agent_runs(
    State(state): State<AppState>,
    Path(task_id): Path<Uuid>,
) -> Result<Json<Vec<TaskAgentRun>>, ServerError> {
    tasks::get_task(&state.db, &task_id)
        .await
        .map_err(|_| ServerError::NotFound("Task not found".into()))?;

    let runs = tasks::list_agent_runs_for_task(&state.db, &task_id)
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;

    Ok(Json(runs))
}
