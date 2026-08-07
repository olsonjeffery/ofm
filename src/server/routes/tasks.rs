use crate::archive;
use crate::auth::AuthUser;
use crate::db::schema::{ActiveAgent, GlobalAgentStatus, Project, Task};
use crate::server::error::ServerError;
use crate::server::state::AppState;
use crate::server::ws::message::{ServerMessage, TopicId, WsTopic, WsTopicKind};
use crate::services;
use crate::worktree::{self, CreateWorktreeResult};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use uuid::Uuid;

const MAX_TITLE_LENGTH: usize = 256;
const MAX_ORIGINAL_REQUEST_LENGTH: usize = 1_001_240;
const MAX_DOC_CONTENT_LENGTH: usize = 1_000_000;

#[derive(Debug, Deserialize)]
pub struct CreateTaskRequest {
    pub project_id: i64,
    pub title: String,
    pub status: Option<String>,
    pub original_request: String,
}

#[derive(Debug, Deserialize)]
struct UpdateTaskRequest {
    title: Option<String>,
    status: Option<String>,
    doc_content: Option<String>,
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
        .route("/active-agents", get(active_agents_handler))
        .route("/agent-status", get(agent_status_handler))
        .route("/{id}", get(get_task).put(update_task).delete(delete_task))
        .nest("/{id}/agent-runs", super::agent_runs::agent_runs_router())
        .nest(
            "/{id}/conversations",
            super::conversations::conversations_router(),
        )
        .route("/{id}/worktree/recreate", post(recreate_worktree_handler))
        .route("/{id}/reset-cap", post(reset_task_cap_handler))
        .route("/{id}/reset-history", post(reset_task_history_handler))
        .route("/{id}/duplicate", post(duplicate_task_handler))
}

/// Fetch a task and verify `auth` may access it (`write=false` → read-only,
/// `write=true` → contributor+). Returns 404 when the task is missing or not
/// accessible, so callers never leak its existence.
pub(crate) async fn authorized_task(
    state: &AppState,
    auth: &AuthUser,
    task_id: i64,
    write: bool,
) -> Result<Task, ServerError> {
    let task = services::tasks::get_task(&state.db, task_id)
        .await
        .map_err(|_| ServerError::NotFound("Task not found".into()))?;
    let has_access = if write {
        services::access::has_task_flow_write_access(&state.db, auth, &task).await
    } else {
        services::access::has_task_flow_access(&state.db, auth, &task).await
    }
    .map_err(|e| ServerError::Internal(e.to_string()))?;
    if !has_access {
        return Err(ServerError::NotFound("Task not found".into()));
    }
    Ok(task)
}

async fn active_agents_handler(
    auth: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<ActiveAgent>>, ServerError> {
    let agents = services::tasks::get_running_agents(&state.db, &auth)
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;
    Ok(Json(agents))
}

async fn agent_status_handler(
    auth: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<GlobalAgentStatus>, ServerError> {
    let status = services::tasks::get_global_agent_status(&state.db, &auth)
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;
    Ok(Json(status))
}

pub async fn create_task(
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

    let project = super::projects::authorized_project(&state, &auth, body.project_id, true).await?;
    let title = body.title.trim().to_string();
    let task = create_task_and_worktree(
        &state,
        &auth,
        &project,
        &title,
        &status,
        &body.original_request,
    )
    .await?;

    Ok((StatusCode::CREATED, Json(task)))
}

/// Shared create-task sequence used by `create_task` and `duplicate_task`:
/// insert the task row, create + register its worktree, seed the archive doc.
async fn create_task_and_worktree(
    state: &AppState,
    auth: &AuthUser,
    project: &Project,
    title: &str,
    status: &str,
    doc: &str,
) -> Result<Task, ServerError> {
    let task = services::tasks::create_task(&state.db, project.id, &auth.user_id, title, status)
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;

    let worktree_result = match worktree::create_worktree(
        &project.repo_folder_path,
        &state.footprint,
        project.id,
        task.id,
        title,
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

    services::tasks::insert_worktree(
        &state.db,
        &Uuid::new_v4(),
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
        .write_task_doc(&doc_path, doc)
        .map_err(|e| ServerError::Internal(format!("failed to seed doc: {e}")))?;

    Ok(task)
}

async fn list_tasks(
    auth: AuthUser,
    State(state): State<AppState>,
    Query(query): Query<ListTasksQuery>,
) -> Result<Json<Vec<Task>>, ServerError> {
    super::projects::authorized_project(&state, &auth, query.project_id, false).await?;
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
    let task = authorized_task(&state, &auth, id, false).await?;

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
            .build_context_prompt(
                &state.footprint,
                w.project_id,
                w.task_id,
                &state.config.agent_host(),
                state.cfg_port,
                std::process::id(),
            )
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
    authorized_task(&state, &auth, id, true).await?;
    if body.title.is_none() && body.status.is_none() && body.doc_content.is_none() {
        return Err(ServerError::BadRequest(
            "at least one field (title, status, doc_content) must be provided".into(),
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

    let task =
        services::tasks::update_task(&state.db, id, body.title.as_deref(), body.status.as_deref())
            .await
            .map_err(|e| {
                if e.to_string().contains("no rows returned") {
                    ServerError::NotFound("Task not found".into())
                } else {
                    ServerError::Internal(e.to_string())
                }
            })?;

    if let Some(ref doc_content) = body.doc_content {
        if doc_content.len() > MAX_DOC_CONTENT_LENGTH {
            return Err(ServerError::BadRequest(
                "doc_content exceeds maximum length".into(),
            ));
        }
        if let Ok(worktree) = services::tasks::get_worktree_by_task(&state.db, id).await {
            let archive = archive::ArchiveRoot::new(std::path::PathBuf::from(&state.archive_root));
            let doc_path = archive.task_doc_path(
                &worktree.project_id.to_string(),
                &worktree.task_id.to_string(),
            );
            if let Err(e) = archive.write_task_doc(&doc_path, doc_content) {
                tracing::warn!("failed to write task doc: {e}");
            }
        }
    }

    broadcast_task_updated(&state, &task).await;

    Ok(Json(task))
}

async fn delete_task(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<axum::http::StatusCode, ServerError> {
    let task = authorized_task(&state, &auth, id, true).await?;
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
            let wt_path = std::path::Path::new(&w.worktree_path);
            let _ = worktree::remove_worktree(rp, wt_path)
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

pub async fn recreate_worktree_handler(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<(StatusCode, Json<CreateWorktreeResult>), ServerError> {
    let task = authorized_task(&state, &auth, id, true).await?;
    let worktree = services::tasks::get_worktree_by_task(&state.db, id)
        .await
        .map_err(|_| ServerError::NotFound("No worktree for task".into()))?;

    let repo = if worktree.repo_path.is_empty() {
        services::projects::get_project(&state.db, task.project_id)
            .await
            .map_err(|_| ServerError::NotFound("Project not found".into()))?
            .repo_folder_path
    } else {
        worktree.repo_path.clone()
    };

    let result = worktree::recreate_worktree(
        &repo,
        &state.footprint,
        task.project_id,
        task.id,
        &worktree.branch,
        &task.title,
    )
    .await
    .map_err(|e| ServerError::Internal(e.to_string()))?;

    broadcast_task_updated(&state, &task).await;

    Ok((StatusCode::OK, Json(result)))
}

/// Broadcast a `task_updated` event on the task topic so open pages reload.
async fn broadcast_task_updated(state: &AppState, task: &Task) {
    let topic = WsTopic {
        kind: WsTopicKind::Task,
        id: TopicId(task.id),
    };
    let msg = ServerMessage::Event {
        topic: topic.clone(),
        event_type: "task_updated".to_string(),
        timestamp: chrono::Utc::now(),
        payload: serde_json::to_value(task).unwrap_or_default(),
        html: None,
    };
    state.ws_bus.broadcast(&topic, msg).await;
}

/// `POST /api/tasks/{id}/reset-cap` — zero `workflow_run_count` and clear
/// `workflow_blocked`. Keeps history, worktree, and task id.
async fn reset_task_cap_handler(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ServerError> {
    authorized_task(&state, &auth, id, true).await?;
    services::tasks::reset_task_cap(&state.db, id)
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;

    let task = services::tasks::get_task(&state.db, id)
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;
    broadcast_task_updated(&state, &task).await;

    Ok(StatusCode::OK)
}

/// `POST /api/tasks/{id}/reset-history` — abort in-flight turns, delete the
/// task's agent runs, conversations, and messages, and reset all workflow
/// flags / run count / status to a fresh `pending`. Keeps task id, title, doc,
/// and worktree.
async fn reset_task_history_handler(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, ServerError> {
    authorized_task(&state, &auth, id, true).await?;

    // Abort any in-flight provider turn before deleting the runs/conversations
    // that the broadcast task would otherwise keep streaming events for.
    let conv_ids: Vec<String> = state
        .db
        .query_raw(
            "SELECT id FROM conversations WHERE task_id = $1",
            hiqlite::params!(id),
        )
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?
        .into_iter()
        .filter_map(|mut row| row.get("id"))
        .collect();
    {
        let sessions = state.active_sessions.lock().await;
        for conv_id in &conv_ids {
            if let Some(provider) = sessions.get(conv_id) {
                let _ = provider.abort_turn().await;
            }
        }
    }

    services::tasks::reset_task_history(&state.db, id)
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;

    let task = services::tasks::get_task(&state.db, id)
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;
    broadcast_task_updated(&state, &task).await;

    crate::orchestration::broadcast_agent_status(&state.ws_bus, "stopped").await;

    Ok(StatusCode::OK)
}

/// `POST /api/tasks/{id}/duplicate` — create a *new* task whose archive doc is
/// a copy of the source doc, with a fresh worktree and zero counters/flags/
/// conversations. Original task untouched.
async fn duplicate_task_handler(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<(StatusCode, Json<Task>), ServerError> {
    let task = authorized_task(&state, &auth, id, true).await?;
    let project = super::projects::authorized_project(&state, &auth, task.project_id, true).await?;

    let source_doc = services::tasks::get_worktree_by_task(&state.db, id)
        .await
        .ok()
        .map(|w| {
            let archive = archive::ArchiveRoot::new(std::path::PathBuf::from(&state.archive_root));
            let doc_path = archive.task_doc_path(&w.project_id.to_string(), &w.task_id.to_string());
            archive.read_task_doc(&doc_path).unwrap_or_default()
        })
        .unwrap_or_default();

    let new_title = format!("{} (copy)", task.title.trim());
    let new_task =
        create_task_and_worktree(&state, &auth, &project, &new_title, "pending", &source_doc)
            .await?;

    Ok((StatusCode::CREATED, Json(new_task)))
}
