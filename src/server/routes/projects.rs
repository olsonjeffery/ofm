use std::path::Path as StdPath;

use axum::{extract::{Path, State}, routing::get, Json, Router};
use serde::Deserialize;
use uuid::Uuid;
use crate::db::schema::Project;
use crate::server::error::ServerError;
use crate::server::state::AppState;
use crate::services;

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
        .route("/{id}", get(get_project).put(update_project).delete(delete_project))
}

async fn create_project(
    State(state): State<AppState>,
    Json(body): Json<CreateProjectRequest>,
) -> Result<(axum::http::StatusCode, Json<Project>), ServerError> {
    if body.name.trim().is_empty() {
        return Err(ServerError::BadRequest("name is required".into()));
    }
    if body.repo_folder_path.trim().is_empty() {
        return Err(ServerError::BadRequest("repo_folder_path is required".into()));
    }
    validate_repo_path(body.repo_folder_path.trim())?;
    let conn = state.db.lock().map_err(|e| ServerError::Internal(e.to_string()))?;
    let project = services::projects::create_project(
        &conn,
        &state.default_user_id,
        body.name.trim(),
        body.repo_folder_path.trim(),
        body.subproject_path.as_deref().map(|s| s.trim()).filter(|s| !s.is_empty()),
    ).map_err(|e| {
        if e.to_string().contains("UNIQUE") {
            ServerError::Conflict("A project with this repository path already exists".into())
        } else {
            ServerError::Internal(e.to_string())
        }
    })?;
    Ok((axum::http::StatusCode::CREATED, Json(project)))
}

async fn list_projects(
    State(state): State<AppState>,
) -> Result<Json<Vec<Project>>, ServerError> {
    let conn = state.db.lock().map_err(|e| ServerError::Internal(e.to_string()))?;
    let projects = services::projects::list_projects(&conn, &state.default_user_id)
        .map_err(|e| ServerError::Internal(e.to_string()))?;
    Ok(Json(projects))
}

async fn get_project(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Project>, ServerError> {
    let conn = state.db.lock().map_err(|e| ServerError::Internal(e.to_string()))?;
    let project = services::projects::get_project(&conn, &id)
        .map_err(|_| ServerError::NotFound("Project not found".into()))?;
    Ok(Json(project))
}

async fn update_project(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateProjectRequest>,
) -> Result<Json<Project>, ServerError> {
    if body.name.as_deref().map_or(false, |n| n.trim().is_empty()) {
        return Err(ServerError::BadRequest("name must not be empty".into()));
    }
    if body.repo_folder_path.as_deref().map_or(false, |r| r.trim().is_empty()) {
        return Err(ServerError::BadRequest("repo_folder_path must not be empty".into()));
    }
    if let Some(ref path) = body.repo_folder_path {
        validate_repo_path(path.trim())?;
    }
    let conn = state.db.lock().map_err(|e| ServerError::Internal(e.to_string()))?;
    let project = services::projects::update_project(
        &conn,
        &id,
        body.name.as_deref().map(|s| s.trim()),
        body.repo_folder_path.as_deref().map(|s| s.trim()),
        body.subproject_path.as_ref().map(|s| s.as_str()),
    ).map_err(|e| {
        if e.to_string().contains("UNIQUE") {
            ServerError::Conflict("A project with this repository path already exists".into())
        } else if e.to_string().contains("returned no rows") {
            ServerError::NotFound("Project not found".into())
        } else {
            ServerError::Internal(e.to_string())
        }
    })?;
    Ok(Json(project))
}

async fn delete_project(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, ServerError> {
    let conn = state.db.lock().map_err(|e| ServerError::Internal(e.to_string()))?;
    let deleted = services::projects::delete_project(&conn, &id)
        .map_err(|e| ServerError::Internal(e.to_string()))?;
    if !deleted {
        return Err(ServerError::NotFound("Project not found".into()));
    }
    Ok(Json(serde_json::json!({ "success": true })))
}

fn validate_repo_path(path: &str) -> Result<(), ServerError> {
    if path.contains("..") {
        return Err(ServerError::BadRequest(
            "repo_folder_path must not contain path traversal sequences".into(),
        ));
    }
    if !StdPath::new(path).has_root() {
        return Err(ServerError::BadRequest(
            "repo_folder_path must be an absolute path".into(),
        ));
    }
    if path.len() > 4096 {
        return Err(ServerError::BadRequest(
            "repo_folder_path is too long".into(),
        ));
    }
    Ok(())
}
