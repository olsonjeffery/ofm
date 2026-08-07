use std::str::FromStr;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::db::schema::{AgentType, Prompt, PromptAssignment, PromptKind, ScopeType};
use crate::server::error::ServerError;
use crate::server::state::AppState;
use crate::services;
use crate::services::prompts::{self, PromptError};

#[derive(Debug, Deserialize)]
struct CreatePromptRequest {
    kind: String,
    title: String,
    content: String,
    tags: Option<Vec<String>>,
    is_shared: Option<bool>,
    children: Option<Vec<Uuid>>,
}

#[derive(Debug, Deserialize)]
struct UpdatePromptRequest {
    title: String,
    content: String,
    tags: Option<Vec<String>>,
    is_shared: Option<bool>,
    children: Option<Vec<Uuid>>,
}

#[derive(Debug, Deserialize)]
struct ValidatePromptRequest {
    content: String,
    tags: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ValidatePromptResponse {
    valid: bool,
    unknown_tokens: Vec<String>,
    invalid_tags: Vec<String>,
}

#[derive(Debug, Serialize)]
struct PromptDetail {
    prompt: Prompt,
    children: Vec<Prompt>,
}

#[derive(Debug, Deserialize)]
struct ListAssignmentsQuery {
    project_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct CreateAssignmentRequest {
    agent_type: String,
    scope_type: String,
    project_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct PreviewQuery {
    task_id: Option<i64>,
    project_id: Option<i64>,
}

#[derive(Debug, Serialize)]
struct PreviewResponse {
    content: String,
}

pub fn prompts_router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_prompts).post(create_prompt))
        .route("/validate", post(validate_prompt_content))
        .route("/assignments", get(list_assignments))
        .route("/assignments/{assignment_id}", delete(delete_assignment))
        .route(
            "/{id}",
            get(get_prompt).put(update_prompt).delete(delete_prompt),
        )
        .route("/{id}/duplicate", post(duplicate_prompt))
        .route("/{id}/assignments", post(create_assignment))
        .route("/{id}/preview", get(preview_prompt))
}

fn map_prompt_error(e: PromptError) -> ServerError {
    match e {
        PromptError::NotFound => ServerError::NotFound("Prompt not found".into()),
        PromptError::StaticImmutable => {
            ServerError::Forbidden("Static prompts cannot be modified".into())
        }
        PromptError::BadRequest(msg) => ServerError::BadRequest(msg),
        PromptError::Validation {
            unknown_tokens,
            invalid_tags,
        } => ServerError::BadRequest(format!(
            "validation failed: unknown tokens {unknown_tokens:?}, invalid tags {invalid_tags:?}"
        )),
        PromptError::Db(msg) => ServerError::Internal(msg),
    }
}

async fn list_prompts(
    auth: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<Prompt>>, ServerError> {
    let prompts = prompts::list_prompts(&state.db, &auth.user_id)
        .await
        .map_err(map_prompt_error)?;
    Ok(Json(prompts))
}

async fn create_prompt(
    auth: AuthUser,
    State(state): State<AppState>,
    Json(body): Json<CreatePromptRequest>,
) -> Result<(StatusCode, Json<Prompt>), ServerError> {
    let kind = PromptKind::from_str(&body.kind).map_err(ServerError::BadRequest)?;
    let prompt = prompts::create_prompt(
        &state.db,
        &auth.user_id,
        kind,
        &body.title,
        &body.content,
        body.tags.unwrap_or_default(),
        body.is_shared.unwrap_or(false),
        body.children.unwrap_or_default(),
    )
    .await
    .map_err(map_prompt_error)?;
    Ok((StatusCode::CREATED, Json(prompt)))
}

async fn get_prompt(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<PromptDetail>, ServerError> {
    let prompt = prompts::get_prompt(&state.db, &id)
        .await
        .map_err(map_prompt_error)?;
    if !prompts::prompt_visible(&prompt, &auth.user_id) {
        return Err(ServerError::NotFound("Prompt not found".into()));
    }
    let children = prompts::get_children(&state.db, &id)
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;
    Ok(Json(PromptDetail { prompt, children }))
}

async fn update_prompt(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdatePromptRequest>,
) -> Result<Json<Prompt>, ServerError> {
    let existing = prompts::get_prompt(&state.db, &id)
        .await
        .map_err(map_prompt_error)?;
    if existing.owner_user_id != Some(auth.user_id) {
        return Err(ServerError::Forbidden(
            "Only the owner can edit this prompt".into(),
        ));
    }
    let prompt = prompts::update_prompt(
        &state.db,
        &id,
        &body.title,
        &body.content,
        body.tags.unwrap_or(existing.tags),
        body.is_shared.unwrap_or(existing.is_shared),
        body.children,
    )
    .await
    .map_err(map_prompt_error)?;
    Ok(Json(prompt))
}

async fn delete_prompt(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ServerError> {
    let existing = prompts::get_prompt(&state.db, &id)
        .await
        .map_err(map_prompt_error)?;
    if existing.owner_user_id != Some(auth.user_id) {
        return Err(ServerError::Forbidden(
            "Only the owner can delete this prompt".into(),
        ));
    }
    prompts::delete_prompt(&state.db, &id)
        .await
        .map_err(map_prompt_error)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn duplicate_prompt(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<(StatusCode, Json<Prompt>), ServerError> {
    let existing = prompts::get_prompt(&state.db, &id)
        .await
        .map_err(map_prompt_error)?;
    if !prompts::prompt_visible(&existing, &auth.user_id) {
        return Err(ServerError::NotFound("Prompt not found".into()));
    }
    let prompt = prompts::duplicate_prompt(&state.db, &auth.user_id, &id)
        .await
        .map_err(map_prompt_error)?;
    Ok((StatusCode::CREATED, Json(prompt)))
}

async fn validate_prompt_content(
    Json(body): Json<ValidatePromptRequest>,
) -> Result<Json<ValidatePromptResponse>, ServerError> {
    let tags = body.tags.unwrap_or_default();
    let (unknown_tokens, invalid_tags) = prompts::validation_report(&body.content, &tags);
    Ok(Json(ValidatePromptResponse {
        valid: unknown_tokens.is_empty() && invalid_tags.is_empty(),
        unknown_tokens,
        invalid_tags,
    }))
}

async fn list_assignments(
    auth: AuthUser,
    State(state): State<AppState>,
    Query(query): Query<ListAssignmentsQuery>,
) -> Result<Json<Vec<PromptAssignment>>, ServerError> {
    if let Some(project_id) = query.project_id {
        super::projects::authorized_project(&state, &auth, project_id, false).await?;
    }
    let assignments = prompts::list_assignments(&state.db, query.project_id)
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;
    Ok(Json(assignments))
}

async fn create_assignment(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<CreateAssignmentRequest>,
) -> Result<(StatusCode, Json<PromptAssignment>), ServerError> {
    let agent_type = AgentType::from_str(&body.agent_type).map_err(ServerError::BadRequest)?;
    let scope_type = ScopeType::from_str(&body.scope_type).map_err(ServerError::BadRequest)?;
    if scope_type == ScopeType::Global && !auth.is_admin {
        return Err(ServerError::Forbidden(
            "Only admins can designate prompts globally".into(),
        ));
    }
    if scope_type == ScopeType::Project {
        let project_id = body.project_id.ok_or_else(|| {
            ServerError::BadRequest("project_id is required for project scope".into())
        })?;
        super::projects::authorized_project(&state, &auth, project_id, true).await?;
    }
    // A prompt the caller cannot even see must not be designable.
    let prompt = prompts::get_prompt(&state.db, &id)
        .await
        .map_err(map_prompt_error)?;
    if !prompts::prompt_visible(&prompt, &auth.user_id) {
        return Err(ServerError::NotFound("Prompt not found".into()));
    }
    prompts::validate_assignment_target(&state.db, &id)
        .await
        .map_err(map_prompt_error)?;
    let assignment = prompts::upsert_assignment(
        &state.db,
        &id,
        &agent_type,
        &scope_type,
        if scope_type == ScopeType::Project {
            body.project_id
        } else {
            None
        },
    )
    .await
    .map_err(map_prompt_error)?;
    Ok((StatusCode::CREATED, Json(assignment)))
}

async fn delete_assignment(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(assignment_id): Path<Uuid>,
) -> Result<StatusCode, ServerError> {
    let assignment = prompts::get_assignment(&state.db, &assignment_id)
        .await
        .map_err(map_prompt_error)?;
    match assignment.scope_type {
        ScopeType::Global | ScopeType::User | ScopeType::UserProject => {
            // Only Global scopes are creatable today; the per-user scopes are
            // future-proofing, and all of them resolve across users' runs, so
            // removal is admin-only like a global designation.
            if !auth.is_admin {
                return Err(ServerError::Forbidden(
                    "Only admins can remove global designations".into(),
                ));
            }
        }
        ScopeType::Project => {
            if let Some(project_id) = assignment.project_id {
                super::projects::authorized_project(&state, &auth, project_id, true).await?;
            }
        }
    }
    prompts::delete_assignment(&state.db, &assignment_id)
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn preview_prompt(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(query): Query<PreviewQuery>,
) -> Result<Json<PreviewResponse>, ServerError> {
    let prompt = prompts::get_prompt(&state.db, &id)
        .await
        .map_err(map_prompt_error)?;
    if !prompts::prompt_visible(&prompt, &auth.user_id) {
        return Err(ServerError::NotFound("Prompt not found".into()));
    }
    let flattened = prompts::flattened_content(&state.db, &prompt)
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;

    let mut vars = crate::prompts::PromptVars::default();
    if let Some(project_id) = query.project_id {
        if let Ok(project) = services::projects::get_project(&state.db, project_id).await {
            if services::access::has_project_access(&state.db, &auth, &project)
                .await
                .unwrap_or(false)
            {
                vars.project_id = project.id.to_string();
                vars.project_name = project.name.clone();
                vars.tags = project.tags.join(", ");
                if let Some(task_id) = query.task_id {
                    if let Ok(task) = services::tasks::get_task(&state.db, task_id).await {
                        if task.project_id == project_id {
                            vars.task_id = task.id.to_string();
                            vars.task_name = task.title.clone();
                        }
                    }
                }
            }
        }
    }
    Ok(Json(PreviewResponse {
        content: crate::prompts::render(&flattened, &vars),
    }))
}
