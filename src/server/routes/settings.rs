use std::collections::HashMap;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::db::schema::UserModelConfig;
use crate::server::state::AppState;
use crate::services::settings::{self, AgentModelSetting};

pub fn settings_router() -> Router<AppState> {
    Router::new()
        .route(
            "/config-body",
            get(list_models_handler).post(create_model_handler),
        )
        .route(
            "/config-body/{id}",
            put(update_model_handler).delete(delete_model_handler),
        )
        .route(
            "/agent-models",
            get(get_agent_models_handler).put(upsert_agent_models_handler),
        )
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

impl ErrorResponse {
    fn new(error: impl Into<String>) -> Self {
        Self {
            error: error.into(),
        }
    }
}

#[derive(Deserialize)]
struct ModelRequest {
    name: String,
    config_body: String,
    #[serde(default)]
    harness: String,
}

async fn list_models_handler(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<UserModelConfig>>, (StatusCode, Json<ErrorResponse>)> {
    settings::list_model_configs(&state.db, auth.user_id)
        .await
        .map(Json)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new(e.to_string())),
            )
        })
}

async fn create_model_handler(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<ModelRequest>,
) -> Result<(StatusCode, Json<UserModelConfig>), (StatusCode, Json<ErrorResponse>)> {
    settings::create_model_config(
        &state.db,
        auth.user_id,
        &body.name,
        &body.config_body,
        &body.harness,
    )
    .await
    .map(|cfg| (StatusCode::CREATED, Json(cfg)))
    .map_err(|e| (StatusCode::BAD_REQUEST, Json(ErrorResponse::new(e))))
}

async fn update_model_handler(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<ModelRequest>,
) -> Result<Json<UserModelConfig>, (StatusCode, Json<ErrorResponse>)> {
    let config_root = std::path::Path::new(&state.config_root);
    match settings::update_model_config(
        &state.db,
        auth.user_id,
        config_root,
        id,
        &body.name,
        &body.config_body,
        &body.harness,
    )
    .await
    {
        Ok(Some(cfg)) => Ok(Json(cfg)),
        Ok(None) => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("config not found")),
        )),
        Err(e) => Err((StatusCode::BAD_REQUEST, Json(ErrorResponse::new(e)))),
    }
}

async fn delete_model_handler(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    let config_root = std::path::Path::new(&state.config_root);
    settings::delete_model_config(&state.db, auth.user_id, config_root, id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new(e)),
            )
        })?;

    Ok(StatusCode::NO_CONTENT)
}

async fn get_agent_models_handler(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<HashMap<String, AgentModelSetting>>, (StatusCode, Json<ErrorResponse>)> {
    settings::get_agent_models(&state.db, auth.user_id)
        .await
        .map(Json)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new(e.to_string())),
            )
        })
}

async fn upsert_agent_models_handler(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(models): Json<HashMap<String, AgentModelSetting>>,
) -> Result<Json<HashMap<String, AgentModelSetting>>, (StatusCode, Json<ErrorResponse>)> {
    let config_root = std::path::Path::new(&state.config_root);
    settings::upsert_agent_models(&state.db, auth.user_id, config_root, models)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(ErrorResponse::new(e))))
}
