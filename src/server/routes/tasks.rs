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
        .route(
            "/{id}",
            get(get_task).put(update_task).delete(delete_task),
        )
}

fn lock_db(state: &AppState) -> Result<std::sync::MutexGuard<'_, rusqlite::Connection>, ServerError> {
    state
        .db
        .lock()
        .map_err(|e| ServerError::Internal(e.to_string()))
}

async fn create_task(
    State(state): State<AppState>,
    Json(body): Json<CreateTaskRequest>,
) -> Result<(StatusCode, Json<Task>), ServerError> {
    if body.title.trim().is_empty() {
        return Err(ServerError::BadRequest("title is required".into()));
    }
    if body.title.len() > MAX_TITLE_LENGTH {
        return Err(ServerError::BadRequest("title must not exceed 200 characters".into()));
    }
    if body.original_request.len() > MAX_ORIGINAL_REQUEST_LENGTH {
        return Err(ServerError::BadRequest("original_request must not exceed 10KB".into()));
    }

    let task_id = Uuid::new_v4();
    let status = body
        .status
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or("pending")
        .to_string();

    let (project, task) = {
        let conn = lock_db(&state)?;
        let project = services::projects::get_project(&conn, &body.project_id)
            .map_err(|_| ServerError::NotFound("Project not found".into()))?;
        let task = services::tasks::create_task(
            &conn,
            &task_id,
            &body.project_id,
            &state.default_user_id,
            body.title.trim(),
            &status,
        )
        .map_err(|e| ServerError::Internal(e.to_string()))?;
        (project, task)
    };

    let int_proj = worktree::uuid_to_u32(&project.id);
    let int_task = worktree::uuid_to_u32(&task_id);

    let worktree_result = worktree::create_worktree(
        &project.repo_folder_path,
        int_proj,
        int_task,
        &body.title,
        None,
    )
    .await;

    let worktree_result = match worktree_result {
        Ok(r) => r,
        Err(e) => {
            let conn = lock_db(&state)?;
            let _ = services::tasks::delete_task(&conn, &task_id);
            return Err(ServerError::Internal(format!(
                "worktree creation failed: {e}"
            )));
        }
    };

    {
        let conn = lock_db(&state)?;
        let worktree_uuid = Uuid::new_v4();
        services::tasks::insert_worktree(
            &conn,
            &worktree_uuid,
            &project.id,
            &task_id,
            int_proj,
            int_task,
            &worktree_result.worktree_path.to_string_lossy(),
            &project.repo_folder_path,
            &worktree_result.branch,
        )
        .map_err(|e| ServerError::Internal(e.to_string()))?;
    }

    let int_proj_str = int_proj.to_string();
    let int_task_str = int_task.to_string();
    let archive = archive::ArchiveRoot::from_config();
    archive
        .ensure_project_archive(&int_proj_str)
        .map_err(|e| ServerError::Internal(e.to_string()))?;
    let doc_path = archive::paths::get_task_doc_path(&int_proj_str, &int_task_str)
        .map_err(|e| ServerError::Internal(e.to_string()))?;
    archive
        .write_task_doc(&doc_path, &body.original_request)
        .map_err(|e| ServerError::Internal(format!("failed to seed doc: {e}")))?;

    Ok((StatusCode::CREATED, Json(task)))
}

async fn list_tasks(
    State(state): State<AppState>,
    Query(query): Query<ListTasksQuery>,
) -> Result<Json<Vec<Task>>, ServerError> {
    let conn = lock_db(&state)?;
    let tasks = services::tasks::list_tasks(&conn, &query.project_id)
        .map_err(|e| ServerError::Internal(e.to_string()))?;
    Ok(Json(tasks))
}

async fn get_task(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<TaskDetailResponse>, ServerError> {
    let (task, worktree) = {
        let conn = lock_db(&state)?;
        let task = services::tasks::get_task(&conn, &id)
            .map_err(|_| ServerError::NotFound("Task not found".into()))?;
        let worktree = services::tasks::get_worktree_by_task(&conn, &id).ok();
        (task, worktree)
    };

    if let Some(w) = worktree {
        let int_proj = w.project_id.to_string();
        let int_task = w.task_id.to_string();
        let archive = archive::ArchiveRoot::from_config();
        let doc_path = archive::paths::get_task_doc_path(&int_proj, &int_task)
            .map_err(|e| ServerError::Internal(e.to_string()))?;
        let doc_content = archive
            .read_task_doc(&doc_path)
            .map_err(|e| ServerError::Internal(e.to_string()))?;
        let doc_content = if doc_content.is_empty() {
            None
        } else {
            Some(doc_content)
        };
        let context_prompt = archive
            .build_context_prompt(&int_proj, &int_task)
            .map_err(|e| ServerError::Internal(e.to_string()))?;
        let context_prompt = if context_prompt.is_empty() {
            None
        } else {
            Some(context_prompt)
        };
        Ok(Json(TaskDetailResponse {
            task,
            doc_content,
            context_prompt,
        }))
    } else {
        Ok(Json(TaskDetailResponse {
            task,
            doc_content: None,
            context_prompt: None,
        }))
    }
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
            return Err(ServerError::BadRequest("title must not exceed 200 characters".into()));
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

    let conn = lock_db(&state)?;
    let task = services::tasks::update_task(
        &conn,
        &id,
        body.title.as_deref(),
        body.status.as_deref(),
    )
    .map_err(|e| {
        if e.to_string().contains("returned no rows") {
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
) -> Result<StatusCode, ServerError> {
    let (task, worktree) = {
        let conn = lock_db(&state)?;
        let task = services::tasks::get_task(&conn, &id)
            .map_err(|_| ServerError::NotFound("Task not found".into()))?;
        let worktree = services::tasks::get_worktree_by_task(&conn, &id).ok();
        (task, worktree)
    };

    if let Some(w) = worktree {
        let repo_path = if w.repo_path.is_empty() {
            let conn = lock_db(&state)?;
            services::projects::get_project(&conn, &task.project_id)
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
        let _ = archive::ArchiveRoot::from_config()
            .delete_task_archive(&w.project_id.to_string(), &w.task_id.to_string())
            .map_err(|e| tracing::warn!("failed to delete archive: {e}"));
    }

    {
        let conn = lock_db(&state)?;
        services::tasks::delete_task(&conn, &id)
            .map_err(|e| ServerError::Internal(e.to_string()))?;
    }

    Ok(StatusCode::NO_CONTENT)
}
