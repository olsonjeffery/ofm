use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::post,
    Router,
};
use uuid::Uuid;

use crate::server::{error::ServerError, require_auth, state::AppState};
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

macro_rules! flag_handler {
    ($name:ident, $mark_fn:expr) => {
        async fn $name(
            State(state): State<AppState>,
            Path(task_id): Path<Uuid>,
            headers: HeaderMap,
        ) -> Result<StatusCode, ServerError> {
            require_auth(&headers, &state)?;
            require_task(&state, &task_id).await?;
            $mark_fn(&state.db, &task_id)
                .await
                .map_err(|e| ServerError::Internal(e.to_string()))?;
            Ok(StatusCode::OK)
        }
    };
}

flag_handler!(complete_plan, tasks::mark_planification_complete);
flag_handler!(complete_workflow, tasks::mark_workflow_complete);
flag_handler!(block_workflow, tasks::mark_task_blocked);
flag_handler!(complete_pr, tasks::mark_pr_agent_complete);
