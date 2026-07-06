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

#[derive(Deserialize)]
struct CreateModelRequest {
    name: String,
    config_body: String,
    #[serde(default)]
    harness: String,
}

#[derive(Deserialize)]
struct UpdateModelRequest {
    name: String,
    config_body: String,
    #[serde(default)]
    harness: String,
}

async fn list_models_handler(
    State(_state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<UserModelConfig>>, (StatusCode, Json<ErrorResponse>)> {
    let db = &_state.db;
    settings::list_model_configs(db, auth.user_id)
        .await
        .map(Json)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: e.to_string(),
                }),
            )
        })
}

async fn create_model_handler(
    State(_state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<CreateModelRequest>,
) -> Result<(StatusCode, Json<UserModelConfig>), (StatusCode, Json<ErrorResponse>)> {
    let db = &_state.db;
    settings::create_model_config(
        db,
        auth.user_id,
        &body.name,
        &body.config_body,
        &body.harness,
    )
    .await
    .map(|cfg| (StatusCode::CREATED, Json(cfg)))
    .map_err(|e| (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e })))
}

async fn update_model_handler(
    State(_state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateModelRequest>,
) -> Result<Json<UserModelConfig>, (StatusCode, Json<ErrorResponse>)> {
    let db = &_state.db;
    match settings::update_model_config(
        db,
        auth.user_id,
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
            Json(ErrorResponse {
                error: "config not found".into(),
            }),
        )),
        Err(e) => Err((StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e }))),
    }
}

async fn delete_model_handler(
    State(_state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    let db = &_state.db;
    settings::delete_model_config(db, auth.user_id, id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error: e }),
            )
        })?;

    Ok(StatusCode::NO_CONTENT)
}

async fn get_agent_models_handler(
    State(_state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<HashMap<String, AgentModelSetting>>, (StatusCode, Json<ErrorResponse>)> {
    let db = &_state.db;
    settings::get_agent_models(db, auth.user_id)
        .await
        .map(Json)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: e.to_string(),
                }),
            )
        })
}

async fn upsert_agent_models_handler(
    State(_state): State<AppState>,
    auth: AuthUser,
    Json(models): Json<HashMap<String, AgentModelSetting>>,
) -> Result<Json<HashMap<String, AgentModelSetting>>, (StatusCode, Json<ErrorResponse>)> {
    let db = &_state.db;
    settings::upsert_agent_models(db, auth.user_id, models)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e })))
}
