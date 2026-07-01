use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::post,
    Router,
};
use uuid::Uuid;

use crate::server::error::ServerError;
use crate::server::state::AppState;
use crate::services::tasks;

pub fn agent_flags_router() -> Router<AppState> {
    Router::new()
        .route("/complete-plan", post(complete_plan))
        .route("/complete-workflow", post(complete_workflow))
        .route("/block-workflow", post(block_workflow))
        .route("/complete-pr", post(complete_pr))
}

async fn require_task(state: &AppState, task_id: &Uuid) -> Result<(), ServerError> {
    tasks::get_task(&state.db, task_id)
        .await
        .map_err(|_| ServerError::NotFound("Task not found".into()))?;
    Ok(())
}

async fn complete_plan(
    State(state): State<AppState>,
    Path(task_id): Path<Uuid>,
) -> Result<StatusCode, ServerError> {
    require_task(&state, &task_id).await?;
    tasks::mark_planification_complete(&state.db, &task_id)
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;
    Ok(StatusCode::OK)
}

async fn complete_workflow(
    State(state): State<AppState>,
    Path(task_id): Path<Uuid>,
) -> Result<StatusCode, ServerError> {
    require_task(&state, &task_id).await?;
    tasks::mark_workflow_complete(&state.db, &task_id)
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;
    Ok(StatusCode::OK)
}

async fn block_workflow(
    State(state): State<AppState>,
    Path(task_id): Path<Uuid>,
) -> Result<StatusCode, ServerError> {
    require_task(&state, &task_id).await?;
    tasks::mark_task_blocked(&state.db, &task_id)
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;
    Ok(StatusCode::OK)
}

async fn complete_pr(
    State(state): State<AppState>,
    Path(task_id): Path<Uuid>,
) -> Result<StatusCode, ServerError> {
    require_task(&state, &task_id).await?;
    tasks::mark_pr_agent_complete(&state.db, &task_id)
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;
    Ok(StatusCode::OK)
}
