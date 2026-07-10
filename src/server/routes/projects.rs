use std::path::Path as StdPath;

use crate::auth::AuthUser;
use crate::db::schema::Project;
use crate::server::error::ServerError;
use crate::server::state::AppState;
use crate::services;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct CreateProjectRequest {
    pub name: String,
    pub repo_folder_path: String,
    pub subproject_path: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateProjectRequest {
    pub name: Option<String>,
    pub repo_folder_path: Option<String>,
    pub subproject_path: Option<String>,
}

pub fn projects_router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_projects).post(create_project))
        .route(
            "/{id}",
            get(get_project).put(update_project).delete(delete_project),
        )
}

async fn create_project(
    auth: AuthUser,
    State(state): State<AppState>,
    Json(body): Json<CreateProjectRequest>,
) -> Result<(StatusCode, Json<Project>), ServerError> {
    if body.name.trim().is_empty() {
        return Err(ServerError::BadRequest("name is required".into()));
    }
    if body.repo_folder_path.trim().is_empty() {
        return Err(ServerError::BadRequest(
            "repo_folder_path is required".into(),
        ));
    }
    validate_repo_path(body.repo_folder_path.trim())?;
    if let Some(ref sp) = body.subproject_path {
        let trimmed = sp.trim();
        if !trimmed.is_empty() {
            validate_subproject_path(trimmed)?;
        }
    }
    let project = services::projects::create_project(
        &state.db,
        &auth.user_id,
        body.name.trim(),
        body.repo_folder_path.trim(),
        body.subproject_path
            .as_deref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty()),
    )
    .await
    .map_err(|e| {
        if e.to_string().contains("UNIQUE") {
            ServerError::Conflict("A project with this repository path already exists".into())
        } else {
            ServerError::Internal(e.to_string())
        }
    })?;
    Ok((StatusCode::CREATED, Json(project)))
}

async fn list_projects(
    auth: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<Project>>, ServerError> {
    let projects = services::projects::list_projects(&state.db, &auth.user_id)
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;
    Ok(Json(projects))
}

async fn get_project(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Project>, ServerError> {
    let project = services::projects::get_project(&state.db, id)
        .await
        .map_err(|_| ServerError::NotFound("Project not found".into()))?;
    if project.user_id != auth.user_id {
        return Err(ServerError::NotFound("Project not found".into()));
    }
    Ok(Json(project))
}

async fn update_project(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateProjectRequest>,
) -> Result<Json<Project>, ServerError> {
    let existing = services::projects::get_project(&state.db, id)
        .await
        .map_err(|_| ServerError::NotFound("Project not found".into()))?;
    if existing.user_id != auth.user_id {
        return Err(ServerError::NotFound("Project not found".into()));
    }
    if body.name.as_deref().is_some_and(|n| n.trim().is_empty()) {
        return Err(ServerError::BadRequest("name must not be empty".into()));
    }
    if body
        .repo_folder_path
        .as_deref()
        .is_some_and(|r| r.trim().is_empty())
    {
        return Err(ServerError::BadRequest(
            "repo_folder_path must not be empty".into(),
        ));
    }
    if let Some(ref path) = body.repo_folder_path {
        validate_repo_path(path.trim())?;
    }
    if let Some(ref sp) = body.subproject_path {
        let trimmed = sp.trim();
        if !trimmed.is_empty() {
            validate_subproject_path(trimmed)?;
        }
    }
    let project = services::projects::update_project(
        &state.db,
        id,
        body.name.as_deref().map(|s| s.trim()),
        body.repo_folder_path.as_deref().map(|s| s.trim()),
        body.subproject_path.as_deref(),
    )
    .await
    .map_err(|e| {
        if e.to_string().contains("UNIQUE") {
            ServerError::Conflict("A project with this repository path already exists".into())
        } else if e.to_string().contains("no rows returned") {
            ServerError::NotFound("Project not found".into())
        } else {
            ServerError::Internal(e.to_string())
        }
    })?;
    Ok(Json(project))
}

async fn delete_project(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, ServerError> {
    let existing = services::projects::get_project(&state.db, id)
        .await
        .map_err(|_| ServerError::NotFound("Project not found".into()))?;
    if existing.user_id != auth.user_id {
        return Err(ServerError::NotFound("Project not found".into()));
    }
    let deleted = services::projects::delete_project(&state.db, id)
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;
    if !deleted {
        return Err(ServerError::NotFound("Project not found".into()));
    }
    Ok(Json(serde_json::json!({ "success": true })))
}

fn validate_no_path_traversal(path: &str, field: &str) -> Result<(), ServerError> {
    if path.contains("..") {
        return Err(ServerError::BadRequest(format!(
            "{field} must not contain path traversal sequences"
        )));
    }
    if path.len() > 4096 {
        return Err(ServerError::BadRequest(format!("{field} is too long")));
    }
    Ok(())
}

fn validate_repo_path(path: &str) -> Result<(), ServerError> {
    validate_no_path_traversal(path, "repo_folder_path")?;
    if !StdPath::new(path).has_root() {
        return Err(ServerError::BadRequest(
            "repo_folder_path must be an absolute path".into(),
        ));
    }
    Ok(())
}

fn validate_subproject_path(path: &str) -> Result<(), ServerError> {
    validate_no_path_traversal(path, "subproject_path")
}
