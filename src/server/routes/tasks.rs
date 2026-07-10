use crate::archive;
use crate::auth::AuthUser;
use crate::db::schema::Task;
use crate::server::error::ServerError;
use crate::server::state::AppState;
use crate::services;
use crate::worktree;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use uuid::Uuid;

const MAX_TITLE_LENGTH: usize = 200;
const MAX_ORIGINAL_REQUEST_LENGTH: usize = 10_240;

#[derive(Debug, Deserialize)]
struct CreateTaskRequest {
    project_id: i64,
    title: String,
    status: Option<String>,
    original_request: String,
}

#[derive(Debug, Deserialize)]
struct UpdateTaskRequest {
    title: Option<String>,
    status: Option<String>,
}

#[derive(Debug, serde::Serialize)]
struct TaskDetailResponse {
    #[serde(flatten)]
    task: Task,
    doc_content: Option<String>,
    context_prompt: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ListTasksQuery {
    project_id: i64,
}

const VALID_STATUSES: &[&str] = &["pending", "in_progress", "in_review", "completed"];

pub fn tasks_router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_tasks).post(create_task))
        .route("/{id}", get(get_task).put(update_task).delete(delete_task))
        .nest("/{id}/agent-runs", super::agent_runs::agent_runs_router())
        .nest("/{id}", super::agent_flags::agent_flags_router())
}

async fn create_task(
    auth: AuthUser,
    State(state): State<AppState>,
    Json(body): Json<CreateTaskRequest>,
) -> Result<(axum::http::StatusCode, Json<Task>), ServerError> {
    if body.title.trim().is_empty() {
        return Err(ServerError::BadRequest("title is required".into()));
    }
    if body.title.len() > MAX_TITLE_LENGTH {
        return Err(ServerError::BadRequest(
            "title must not exceed 200 characters".into(),
        ));
    }
    if body.original_request.len() > MAX_ORIGINAL_REQUEST_LENGTH {
        return Err(ServerError::BadRequest(
            "original_request must not exceed 10KB".into(),
        ));
    }

    let status = body
        .status
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or("pending")
        .to_string();

    let project = services::projects::get_project(&state.db, body.project_id)
        .await
        .map_err(|_| ServerError::NotFound("Project not found".into()))?;
    let task = services::tasks::create_task(
        &state.db,
        body.project_id,
        &auth.user_id,
        body.title.trim(),
        &status,
    )
    .await
    .map_err(|e| ServerError::Internal(e.to_string()))?;

    let worktree_result = match worktree::create_worktree(
        &project.repo_folder_path,
        project.id,
        task.id,
        &body.title,
        None,
    )
    .await
    {
        Ok(r) => r,
        Err(e) => {
            let _ = services::tasks::delete_task(&state.db, task.id).await;
            return Err(ServerError::Internal(format!(
                "worktree creation failed: {}",
                e
            )));
        }
    };

    let worktree_uuid = Uuid::new_v4();
    services::tasks::insert_worktree(
        &state.db,
        &worktree_uuid,
        project.id,
        task.id,
        &worktree_result.worktree_path.to_string_lossy(),
        &project.repo_folder_path,
        &worktree_result.branch,
    )
    .await
    .map_err(|e| ServerError::Internal(e.to_string()))?;

    let proj_str = project.id.to_string();
    let task_str = task.id.to_string();
    let archive = archive::ArchiveRoot::new(std::path::PathBuf::from(&state.archive_root));
    archive
        .ensure_project_archive(&proj_str)
        .map_err(|e| ServerError::Internal(e.to_string()))?;
    let doc_path = archive.task_doc_path(&proj_str, &task_str);
    archive
        .write_task_doc(&doc_path, &body.original_request)
        .map_err(|e| ServerError::Internal(format!("failed to seed doc: {e}")))?;

    Ok((StatusCode::CREATED, Json(task)))
}

async fn list_tasks(
    auth: AuthUser,
    State(state): State<AppState>,
    Query(query): Query<ListTasksQuery>,
) -> Result<Json<Vec<Task>>, ServerError> {
    let project = services::projects::get_project(&state.db, query.project_id)
        .await
        .map_err(|_| ServerError::NotFound("Project not found".into()))?;
    if project.user_id != auth.user_id {
        return Err(ServerError::NotFound("Project not found".into()));
    }
    let tasks = services::tasks::list_tasks(&state.db, query.project_id)
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;
    Ok(Json(tasks))
}

async fn get_task(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<TaskDetailResponse>, ServerError> {
    let task = services::tasks::get_task(&state.db, id)
        .await
        .map_err(|_| ServerError::NotFound("Task not found".into()))?;
    if task.user_id != auth.user_id {
        return Err(ServerError::NotFound("Task not found".into()));
    }

    let worktree = services::tasks::get_worktree_by_task(&state.db, id)
        .await
        .ok();

    let (doc_content, context_prompt) = if let Some(w) = worktree {
        let archive = archive::ArchiveRoot::new(std::path::PathBuf::from(&state.archive_root));
        let proj_str = w.project_id.to_string();
        let task_str = w.task_id.to_string();
        let doc_path = archive.task_doc_path(&proj_str, &task_str);
        let doc = archive
            .read_task_doc(&doc_path)
            .map_err(|e| ServerError::Internal(e.to_string()))?;
        let ctx = archive
            .build_context_prompt(&proj_str, &task_str)
            .map_err(|e| ServerError::Internal(e.to_string()))?;
        (
            (!doc.is_empty()).then_some(doc),
            (!ctx.is_empty()).then_some(ctx),
        )
    } else {
        (None, None)
    };

    Ok(Json(TaskDetailResponse {
        task,
        doc_content,
        context_prompt,
    }))
}

async fn update_task(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateTaskRequest>,
) -> Result<Json<Task>, ServerError> {
    let existing = services::tasks::get_task(&state.db, id)
        .await
        .map_err(|_| ServerError::NotFound("Task not found".into()))?;
    if existing.user_id != auth.user_id {
        return Err(ServerError::NotFound("Task not found".into()));
    }
    if body.title.is_none() && body.status.is_none() {
        return Err(ServerError::BadRequest(
            "at least one field (title, status) must be provided".into(),
        ));
    }

    if let Some(ref title) = body.title {
        if title.len() > MAX_TITLE_LENGTH {
            return Err(ServerError::BadRequest(
                "title must not exceed 200 characters".into(),
            ));
        }
    }

    if let Some(ref status) = body.status {
        if !VALID_STATUSES.contains(&status.as_str()) {
            return Err(ServerError::BadRequest(format!(
                "invalid status '{}': must be one of {:?}",
                status, VALID_STATUSES
            )));
        }
    }

    let task = services::tasks::update_task(
        &state.db,
        id,
        body.title.as_deref(),
        body.status.as_deref(),
    )
    .await
    .map_err(|e| {
        if e.to_string().contains("no rows returned") {
            ServerError::NotFound("Task not found".into())
        } else {
            ServerError::Internal(e.to_string())
        }
    })?;
    Ok(Json(task))
}

async fn delete_task(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<axum::http::StatusCode, ServerError> {
    let task = services::tasks::get_task(&state.db, id)
        .await
        .map_err(|_| ServerError::NotFound("Task not found".into()))?;
    if task.user_id != auth.user_id {
        return Err(ServerError::NotFound("Task not found".into()));
    }
    let worktree = services::tasks::get_worktree_by_task(&state.db, id)
        .await
        .ok();

    if let Some(w) = worktree {
        let repo = if w.repo_path.is_empty() {
            services::projects::get_project(&state.db, task.project_id)
                .await
                .ok()
                .map(|p| p.repo_folder_path)
        } else {
            Some(w.repo_path)
        };
        if let Some(ref rp) = repo {
            let _ = worktree::remove_worktree(rp, w.project_id, w.task_id)
                .await
                .map_err(|e| tracing::warn!("failed to remove worktree: {e}"));
        }
        let _ = archive::ArchiveRoot::new(std::path::PathBuf::from(&state.archive_root))
            .delete_task_archive(&w.project_id.to_string(), &w.task_id.to_string())
            .map_err(|e| tracing::warn!("failed to delete archive: {e}"));
    }

    services::tasks::delete_task(&state.db, id)
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;

    Ok(StatusCode::NO_CONTENT)
}
