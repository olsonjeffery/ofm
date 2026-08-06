use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::db::schema::UserModelConfig;
use crate::providers::rig_config::RigProviderConfig;
use crate::server::state::AppState;
use crate::services::export_import;
use crate::services::settings::{self, AgentModelSetting, RigProviderWithConfig};

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
        .route(
            "/rig-providers",
            get(list_rig_providers_handler).post(create_rig_provider_handler),
        )
        .route(
            "/rig-providers/{id}",
            get(get_rig_provider_handler)
                .put(update_rig_provider_handler)
                .delete(delete_rig_provider_handler),
        )
        .route(
            "/rig-providers/{id}/models",
            get(get_rig_provider_models_handler),
        )
        .route("/export", get(export_handler))
        .route(
            "/import/preview",
            post(import_preview_handler)
                .layer(axum::extract::DefaultBodyLimit::max(1024 * 1024 * 10)),
        )
        .route(
            "/import/execute",
            post(import_execute_handler)
                .layer(axum::extract::DefaultBodyLimit::max(1024 * 1024 * 10)),
        )
}

#[derive(Debug, Serialize)]
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
    let configs = crate::services::access::list_accessible_model_configs(&state.db, &auth)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new(e.to_string())),
            )
        })?;

    Ok(Json(configs))
}

async fn create_model_handler(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<ModelRequest>,
) -> Result<(StatusCode, Json<UserModelConfig>), (StatusCode, Json<ErrorResponse>)> {
    let config_root = std::path::Path::new(&state.config_root);
    settings::create_model_config(
        &state.db,
        auth.user_id,
        config_root,
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

    let existing = settings::get_model_config_by_id(&state.db, id)
        .await
        .map_err(|_e| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse::new("config not found")),
            )
        })?;
    let authorized =
        crate::services::access::has_model_config_write_access(&state.db, &auth, &existing)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse::new(e.to_string())),
                )
            })?;
    if !authorized {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("config not found")),
        ));
    }

    match settings::update_model_config_by_id(
        &state.db,
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

    // DELETE is idempotent: a config that does not exist — or is not visible
    // to the caller — is a no-op returning 204.
    let existing = settings::get_model_config_by_id(&state.db, id).await.ok();
    if let Some(existing) = existing {
        let authorized =
            crate::services::access::has_model_config_write_access(&state.db, &auth, &existing)
                .await
                .map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ErrorResponse::new(e.to_string())),
                    )
                })?;
        if !authorized {
            return Ok(StatusCode::NO_CONTENT);
        }
        settings::delete_model_config_by_id(&state.db, config_root, id)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse::new(e)),
                )
            })?;
    }

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

// ── Rig provider CRUD handlers ───────────────────────────────────────────────

async fn list_rig_providers_handler(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<RigProviderWithConfig>>, (StatusCode, Json<ErrorResponse>)> {
    settings::list_rig_providers(&state.db, auth.user_id)
        .await
        .map(Json)
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new(e)),
            )
        })
}

async fn create_rig_provider_handler(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(cfg): Json<RigProviderConfig>,
) -> Result<(StatusCode, Json<RigProviderWithConfig>), (StatusCode, Json<ErrorResponse>)> {
    let config_root = std::path::Path::new(&state.config_root);
    settings::create_rig_provider(&state.db, auth.user_id, config_root, cfg)
        .await
        .map(|provider| (StatusCode::CREATED, Json(provider)))
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(ErrorResponse::new(e))))
}

async fn update_rig_provider_handler(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(cfg): Json<RigProviderConfig>,
) -> Result<Json<RigProviderWithConfig>, (StatusCode, Json<ErrorResponse>)> {
    let config_root = std::path::Path::new(&state.config_root);
    match settings::update_rig_provider(&state.db, auth.user_id, config_root, id, cfg).await {
        Ok(Some(provider)) => Ok(Json(provider)),
        Ok(None) => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("rig provider not found")),
        )),
        Err(e) => Err((StatusCode::BAD_REQUEST, Json(ErrorResponse::new(e)))),
    }
}

async fn get_rig_provider_handler(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<RigProviderWithConfig>, (StatusCode, Json<ErrorResponse>)> {
    match settings::get_rig_provider(&state.db, auth.user_id, id).await {
        Ok(Some(provider)) => Ok(Json(provider)),
        Ok(None) => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("rig provider not found")),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::new(e)),
        )),
    }
}

async fn delete_rig_provider_handler(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    let config_root = std::path::Path::new(&state.config_root);
    let deleted = settings::delete_rig_provider(&state.db, auth.user_id, config_root, id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new(e)),
            )
        })?;
    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::new("rig provider not found")),
        ))
    }
}

async fn get_rig_provider_models_handler(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<String>>, (StatusCode, Json<ErrorResponse>)> {
    settings::get_rig_provider_models(&state.db, auth.user_id, id)
        .await
        .map(Json)
        .map_err(|e| match e.as_str() {
            "rig provider not found" => (StatusCode::NOT_FOUND, Json(ErrorResponse::new(e))),
            _ => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new(e)),
            ),
        })
}

// ── Export handler ──────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ExportQuery {
    project_ids: String,
}

async fn export_handler(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<ExportQuery>,
) -> Result<Json<export_import::ExportPayload>, (StatusCode, Json<ErrorResponse>)> {
    let ids: Vec<i64> = query
        .project_ids
        .split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();

    if ids.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new(
                "at least one valid project_id is required",
            )),
        ));
    }

    let payload = export_import::build_export(
        &state.db,
        &state.archive_root,
        auth.user_id,
        &auth.username,
        &ids,
    )
    .await
    .map_err(|e| (StatusCode::BAD_REQUEST, Json(ErrorResponse::new(e))))?;

    Ok(Json(payload))
}

// ── Import preview handler ──────────────────────────────────────────────────

async fn import_preview_handler(
    body: String,
) -> Result<Json<export_import::ImportPreview>, (StatusCode, Json<ErrorResponse>)> {
    let preview = export_import::preview_import(&body)
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(ErrorResponse::new(e))))?;
    Ok(Json(preview))
}

// ── Import execute handler ──────────────────────────────────────────────────

async fn import_execute_handler(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(request): Json<export_import::ImportExecuteRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    //export_import::execute_import(&state.db, &state.archive_root, auth.user_id, request)
    export_import::execute_import(&state, &auth, request)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(ErrorResponse::new(e))))?;

    Ok(Json(serde_json::json!({ "success": true })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::OfmConfig;
    use crate::providers::rig_config::{ModelListMode, RigProviderConfig, RigVendor};
    use crate::server::ws::bus::BroadcastBus;
    use axum::extract::State;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tempfile::TempDir;
    use tokio::sync::Mutex;

    struct TestCtx {
        state: AppState,
        auth: AuthUser,
        _tmp: TempDir,
    }

    async fn setup() -> TestCtx {
        let tmp = TempDir::new().unwrap();
        let config = hiqlite::NodeConfig {
            node_id: 1,
            nodes: vec![hiqlite::Node {
                id: 1,
                addr_raft: "127.0.0.1:0".into(),
                addr_api: "127.0.0.1:0".into(),
            }],
            data_dir: tmp.path().to_str().unwrap().to_string().into(),
            secret_raft: "test-raft-secret-123".into(),
            secret_api: "test-api-secret-123".into(),
            ..Default::default()
        };
        let client = hiqlite::start_node(config).await.unwrap();
        client.wait_until_healthy_db().await;
        crate::db::run_migrations(&client).await.unwrap();
        let user_id = crate::db::ensure_default_user(&client).await.unwrap();

        let state = AppState {
            cfg_port: 0,
            rauthy_port: None,
            db: client,
            default_user_id: user_id,
            footprint: tmp.path().to_str().unwrap().to_string(),
            archive_root: "storage/".into(),
            config_root: tmp.path().to_str().unwrap().to_string(),
            active_sessions: Arc::new(Mutex::new(HashMap::<
                String,
                Box<dyn crate::providers::LlmProvider>,
            >::new())),
            oidc_provider: None,
            pkce_store: Arc::new(Mutex::new(HashMap::new())),
            cookie_key: cookie::Key::generate(),
            api_key_pepper: b"test_pepper".to_vec(),
            ws_bus: BroadcastBus::new(),
            config: OfmConfig::default(),
        };
        let auth = AuthUser {
            user_id,
            username: "admin@localhost".into(),
            oidc_subject: None,
            is_admin: true,
            is_technical: true,
        };
        TestCtx {
            state,
            auth,
            _tmp: tmp,
        }
    }

    fn sample_cfg() -> RigProviderConfig {
        RigProviderConfig {
            name: "route-test".into(),
            vendor: RigVendor::OpenAi,
            base_url: None,
            api_key: Some("sk-123".into()),
            model_list_mode: ModelListMode::Manual(vec!["gpt-4".into()]),
            models: vec!["gpt-4".into()],
        }
    }

    #[tokio::test]
    async fn test_rig_provider_handlers_crud() {
        let ctx = setup().await;

        // Create → 201 CREATED
        let created = create_rig_provider_handler(
            State(ctx.state.clone()),
            ctx.auth.clone(),
            Json(sample_cfg()),
        )
        .await
        .expect("create should succeed");
        assert_eq!(created.0, StatusCode::CREATED);
        let provider = created.1 .0;
        assert_eq!(provider.name, "route-test");
        assert_eq!(provider.config.vendor, RigVendor::OpenAi);

        // List → 1 provider
        let listed = list_rig_providers_handler(State(ctx.state.clone()), ctx.auth.clone())
            .await
            .expect("list should succeed");
        assert_eq!(listed.0.len(), 1);
        assert_eq!(listed.0[0].id, provider.id);

        // GET by id → the same provider
        let fetched = get_rig_provider_handler(
            State(ctx.state.clone()),
            ctx.auth.clone(),
            Path(provider.id),
        )
        .await
        .expect("get should succeed");
        assert_eq!(fetched.0.id, provider.id);

        // Models endpoint → manual model list
        let models = get_rig_provider_models_handler(
            State(ctx.state.clone()),
            ctx.auth.clone(),
            Path(provider.id),
        )
        .await
        .expect("models should succeed");
        assert_eq!(models.0, vec!["gpt-4"]);

        // Update → renamed
        let mut cfg = sample_cfg();
        cfg.name = "renamed".into();
        cfg.model_list_mode = ModelListMode::Manual(vec!["gpt-4".into(), "gpt-4o".into()]);
        let updated = update_rig_provider_handler(
            State(ctx.state.clone()),
            ctx.auth.clone(),
            Path(provider.id),
            Json(cfg),
        )
        .await
        .expect("update should succeed");
        assert_eq!(updated.0.name, "renamed");

        // Delete → 204
        let deleted = delete_rig_provider_handler(
            State(ctx.state.clone()),
            ctx.auth.clone(),
            Path(provider.id),
        )
        .await
        .expect("delete should succeed");
        assert_eq!(deleted, StatusCode::NO_CONTENT);

        // List → empty
        let listed = list_rig_providers_handler(State(ctx.state.clone()), ctx.auth.clone())
            .await
            .expect("list should succeed");
        assert!(listed.0.is_empty());
    }

    #[tokio::test]
    async fn test_rig_provider_handlers_invalid_and_not_found() {
        let ctx = setup().await;

        // Invalid body (manual mode, no models) → 400
        let mut bad = sample_cfg();
        bad.model_list_mode = ModelListMode::Manual(vec![]);
        let result =
            create_rig_provider_handler(State(ctx.state.clone()), ctx.auth.clone(), Json(bad))
                .await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::BAD_REQUEST);

        // Update unknown id → 404
        let result = update_rig_provider_handler(
            State(ctx.state.clone()),
            ctx.auth.clone(),
            Path(Uuid::new_v4()),
            Json(sample_cfg()),
        )
        .await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::NOT_FOUND);

        // GET unknown id → 404
        let result = get_rig_provider_handler(
            State(ctx.state.clone()),
            ctx.auth.clone(),
            Path(Uuid::new_v4()),
        )
        .await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::NOT_FOUND);

        // Delete unknown id → 404
        let result = delete_rig_provider_handler(
            State(ctx.state.clone()),
            ctx.auth.clone(),
            Path(Uuid::new_v4()),
        )
        .await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::NOT_FOUND);

        // Models for unknown id → 404
        let result = get_rig_provider_models_handler(
            State(ctx.state.clone()),
            ctx.auth.clone(),
            Path(Uuid::new_v4()),
        )
        .await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().0, StatusCode::NOT_FOUND);
    }
}
