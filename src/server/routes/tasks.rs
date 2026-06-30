use crate::archive;
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
    project_id: Uuid,
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
    project_id: Uuid,
}

const VALID_STATUSES: &[&str] = &["pending", "in_progress", "in_review", "completed"];

pub fn tasks_router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_tasks).post(create_task))
        .route("/{id}", get(get_task).put(update_task).delete(delete_task))
        .nest("/{id}/agent-runs", super::agent_runs::agent_runs_router())
}

async fn create_task(
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

    let task_id = Uuid::new_v4();
    let status = body
        .status
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or("pending")
        .to_string();

    let project = services::projects::get_project(&state.db, &body.project_id)
        .await
        .map_err(|_| ServerError::NotFound("Project not found".into()))?;
    let task = services::tasks::create_task(
        &state.db,
        &task_id,
        &body.project_id,
        &state.default_user_id,
        body.title.trim(),
        &status,
    )
    .await
    .map_err(|e| ServerError::Internal(e.to_string()))?;

    let int_proj = worktree::uuid_to_u32(&project.id);
    let int_task = worktree::uuid_to_u32(&task_id);

    let worktree_result = match worktree::create_worktree(
        &project.repo_folder_path,
        int_proj,
        int_task,
        &body.title,
        None,
    )
    .await
    {
        Ok(r) => r,
        Err(e) => {
            let err_msg = format!("worktree creation failed: {}", e);
            let _ = services::tasks::delete_task(&state.db, &task_id).await;
            return Err(ServerError::Internal(err_msg));
        }
    };

    let worktree_uuid = Uuid::new_v4();
    services::tasks::insert_worktree(
        &state.db,
        &worktree_uuid,
        &project.id,
        &task_id,
        int_proj,
        int_task,
        &worktree_result.worktree_path.to_string_lossy(),
        &project.repo_folder_path,
        &worktree_result.branch,
    )
    .await
    .map_err(|e| ServerError::Internal(e.to_string()))?;

    let int_proj_str = int_proj.to_string();
    let int_task_str = int_task.to_string();
    let archive = archive::ArchiveRoot::new(std::path::PathBuf::from(&state.archive_root));
    archive
        .ensure_project_archive(&int_proj_str)
        .map_err(|e| ServerError::Internal(e.to_string()))?;
    let doc_path = archive.task_doc_path(&int_proj_str, &int_task_str);
    archive
        .write_task_doc(&doc_path, &body.original_request)
        .map_err(|e| ServerError::Internal(format!("failed to seed doc: {e}")))?;

    Ok((StatusCode::CREATED, Json(task)))
}

async fn list_tasks(
    State(state): State<AppState>,
    Query(query): Query<ListTasksQuery>,
) -> Result<Json<Vec<Task>>, ServerError> {
    let tasks = services::tasks::list_tasks(&state.db, &query.project_id)
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;
    Ok(Json(tasks))
}

async fn get_task(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<TaskDetailResponse>, ServerError> {
    let task = services::tasks::get_task(&state.db, &id)
        .await
        .map_err(|_| ServerError::NotFound("Task not found".into()))?;

    let worktree = services::tasks::get_worktree_by_task(&state.db, &id)
        .await
        .ok();

    let (doc_content, context_prompt) = if let Some(w) = worktree {
        let archive = archive::ArchiveRoot::new(std::path::PathBuf::from(&state.archive_root));
        let doc_path = archive.task_doc_path(&w.project_id.to_string(), &w.task_id.to_string());
        let doc = archive
            .read_task_doc(&doc_path)
            .map_err(|e| ServerError::Internal(e.to_string()))?;
        let ctx = archive
            .build_context_prompt(&w.project_id.to_string(), &w.task_id.to_string())
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
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateTaskRequest>,
) -> Result<Json<Task>, ServerError> {
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
        &id,
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
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<axum::http::StatusCode, ServerError> {
    let task = services::tasks::get_task(&state.db, &id)
        .await
        .map_err(|_| ServerError::NotFound("Task not found".into()))?;
    let worktree = services::tasks::get_worktree_by_task(&state.db, &id)
        .await
        .ok();

    if let Some(w) = worktree {
        let repo_path = if w.repo_path.is_empty() {
            services::projects::get_project(&state.db, &task.project_id)
                .await
                .ok()
                .map(|p| p.repo_folder_path)
        } else {
            Some(w.repo_path)
        };
        if let Some(ref rp) = repo_path {
            let _ = worktree::remove_worktree(rp, w.project_id, w.task_id)
                .await
                .map_err(|e| tracing::warn!("failed to remove worktree: {e}"));
        }
        let _ = archive::ArchiveRoot::new(std::path::PathBuf::from(&state.archive_root))
            .delete_task_archive(&w.project_id.to_string(), &w.task_id.to_string())
            .map_err(|e| tracing::warn!("failed to delete archive: {e}"));
    }

    services::tasks::delete_task(&state.db, &id)
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;

    Ok(StatusCode::NO_CONTENT)
}
