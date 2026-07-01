use std::path::PathBuf;
use std::str::FromStr;

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::db::schema::{AgentHarnessConfig, AgentType, ScopeType};
use crate::providers::config::ProviderConfigDir;
use crate::providers::registry;
use crate::server::{error::ServerError, require_auth, state::AppState};
use crate::services;

#[derive(Debug, Deserialize)]
struct CreateAgentConfigRequest {
    agent_type: String,
    harness: String,
    provider_config_ref: String,
    scope_type: Option<String>,
    model: Option<String>,
    effort: Option<String>,
}

#[derive(Debug, Serialize)]
struct ProviderConfigFile {
    name: String,
    harness: String,
}

#[derive(Debug, Deserialize)]
struct ModelListQuery {
    config_ref: String,
}

pub fn agent_configs_router() -> Router<AppState> {
    Router::new()
        .route("/", post(create_agent_config).get(list_agent_configs))
        .route("/{config_id}", delete(delete_agent_config))
        .route("/{config_id}/model", post(select_model))
}

async fn create_agent_config(
    State(state): State<AppState>,
    Path(_project_id): Path<Uuid>,
    headers: HeaderMap,
    Json(body): Json<CreateAgentConfigRequest>,
) -> Result<(StatusCode, Json<AgentHarnessConfig>), ServerError> {
    require_auth(&headers, &state)?;

    if body.provider_config_ref.contains("..") || body.provider_config_ref.contains('/') {
        return Err(ServerError::BadRequest(
            "provider_config_ref must be a simple filename (no path components)".into(),
        ));
    }

    let agent_type = AgentType::from_str(&body.agent_type).map_err(ServerError::BadRequest)?;
    let scope_type: ScopeType = body
        .scope_type
        .as_deref()
        .unwrap_or("global")
        .parse()
        .map_err(|e: String| ServerError::BadRequest(e))?;

    let config = services::agent_configs::create_or_update_agent_config(
        &state.db,
        &agent_type,
        &body.harness,
        &body.provider_config_ref,
        &scope_type,
        None,
        Some(&_project_id),
        body.model.as_deref(),
        body.effort.as_deref(),
    )
    .await
    .map_err(|e| ServerError::Internal(e.to_string()))?;

    Ok((StatusCode::CREATED, Json(config)))
}

async fn list_agent_configs(
    State(state): State<AppState>,
    Path(project_id): Path<Uuid>,
) -> Result<Json<Vec<AgentHarnessConfig>>, ServerError> {
    let configs = services::agent_configs::list_agent_configs(&state.db, Some(&project_id))
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;
    Ok(Json(configs))
}

async fn delete_agent_config(
    State(state): State<AppState>,
    Path((_project_id, config_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<StatusCode, ServerError> {
    require_auth(&headers, &state)?;

    let deleted = services::agent_configs::delete_agent_config(&state.db, &config_id)
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;
    if !deleted {
        return Err(ServerError::NotFound("Agent config not found".into()));
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
struct SelectModelRequest {
    model: String,
}

async fn select_model(
    State(state): State<AppState>,
    Path((_project_id, config_id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    Json(body): Json<SelectModelRequest>,
) -> Result<Json<AgentHarnessConfig>, ServerError> {
    require_auth(&headers, &state)?;

    let config = services::agent_configs::update_agent_config_model(
        &state.db,
        &config_id,
        &body.model,
    )
    .await
    .map_err(|e| ServerError::Internal(e.to_string()))?;

    Ok(Json(config))
}

pub fn provider_configs_router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_provider_config_files))
        .route("/models", get(get_models_for_config_ref))
}

async fn list_provider_config_files(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<ProviderConfigFile>>, ServerError> {
    require_auth(&headers, &state)?;
    let cfg_dir = ProviderConfigDir::new(&PathBuf::from(&state.config_root));
    let names = cfg_dir.list_configs().map_err(|e| ServerError::Internal(e.to_string()))?;
    let files: Vec<ProviderConfigFile> = names
        .into_iter()
        .map(|name| {
            let harness = if name.ends_with(".yaml") || name.ends_with(".yml") {
                "oh-my-pi"
            } else {
                "opencode"
            };
            ProviderConfigFile {
                name,
                harness: harness.to_string(),
            }
        })
        .collect();
    Ok(Json(files))
}

fn validate_config_ref(name: &str) -> Result<(), ServerError> {
    if name.contains("..") || name.starts_with('/') || name.contains('/') {
        return Err(ServerError::BadRequest(
            "config_ref must be a simple filename (no path components)".into(),
        ));
    }
    Ok(())
}

async fn get_models_for_config_ref(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ModelListQuery>,
) -> Result<Json<Vec<String>>, ServerError> {
    require_auth(&headers, &state)?;
    validate_config_ref(&query.config_ref)?;
    let config_root = PathBuf::from(&state.config_root);
    let models = registry::get_models_for_config(&config_root, &query.config_ref)
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;
    Ok(Json(models))
}
