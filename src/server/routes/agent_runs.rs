use std::path::PathBuf;
use std::str::FromStr;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::post,
    Json, Router,
};
use serde::Deserialize;

use crate::db::schema::{AgentType, TaskAgentRun};
use crate::orchestration;
use crate::orchestration::guards;
use crate::providers;
use crate::providers::registry;
use crate::server::{error::ServerError, state::AppState};
use crate::services::session;
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
    Path(task_id): Path<i64>,
    Json(body): Json<StartAgentRunRequest>,
) -> Result<(StatusCode, Json<TaskAgentRun>), ServerError> {
    let agent_type = AgentType::from_str(&body.agent_type).map_err(ServerError::BadRequest)?;

    let task = tasks::get_task(&state.db, task_id)
        .await
        .map_err(|_| ServerError::NotFound("Task not found".into()))?;

    guards::one_running_per_task(&state.db, task_id).await?;

    guards::iteration_cap(&task)?;

    tasks::increment_workflow_run_count(&state.db, task_id)
        .await
        .map_err(orchestration::internal_err)?;

    // Phase 8: Resolve provider config (graceful fallback if no config exists)
    let config_root = PathBuf::from(&state.config_root);
    let harness_config = registry::resolve_harness_config(
        &state.db,
        &agent_type,
        Some(&task.user_id),
        Some(task.project_id),
    )
    .await;

    let (model, effort) = match &harness_config {
        Ok(cfg) => (
            cfg.model.as_deref().unwrap_or("default").to_string(),
            cfg.effort.as_deref().unwrap_or("balanced").to_string(),
        ),
        Err(_) => {
            tracing::warn!("No provider config found for {agent_type}, using defaults");
            ("default".to_string(), "balanced".to_string())
        }
    };

    let session_result = session::start_session(&state.db, task_id, &model, &effort, agent_type)
        .await
        .map_err(|e| match &e {
            hiqlite::Error::ConstraintViolation(_) => {
                ServerError::Conflict("an agent is already running for this task".into())
            }
            _ => ServerError::Internal(e.to_string()),
        })?;

    if let Ok(cfg) = &harness_config {
        // Start and store provider
        match registry::resolve_provider(cfg, std::path::Path::new("omp"), &config_root).await {
            Ok(mut provider) => {
                let working_dir = std::path::Path::new("/tmp");
                match provider.start(working_dir).await {
                    Ok(()) => {
                        state
                            .active_sessions
                            .lock()
                            .await
                            .insert(session_result.conversation_id.to_string(), provider);
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
        let title_config = cfg.clone();
        let conv_id = session_result.conversation_id;
        let task_title = task.title.clone();
        tokio::spawn(async move {
            providers::generate_conversation_title(
                &db,
                &cfg_root,
                &title_config,
                conv_id,
                &task_title,
            )
            .await;
        });
    }

    let run = tasks::get_agent_run_by_conversation(&state.db, &session_result.conversation_id)
        .await
        .map_err(orchestration::internal_err)?;

    Ok((StatusCode::CREATED, Json(run)))
}

async fn list_agent_runs(
    State(state): State<AppState>,
    Path(task_id): Path<i64>,
) -> Result<Json<Vec<TaskAgentRun>>, ServerError> {
    tasks::get_task(&state.db, task_id)
        .await
        .map_err(|_| ServerError::NotFound("Task not found".into()))?;

    let runs = tasks::list_agent_runs_for_task(&state.db, task_id)
        .await
        .map_err(orchestration::internal_err)?;

    Ok(Json(runs))
}
