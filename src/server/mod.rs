pub mod error;
pub mod routes;
pub mod state;

use axum::http::HeaderMap;
use axum::{extract::DefaultBodyLimit, routing::get, Router};

use crate::server::error::ServerError;
use crate::server::state::AppState;

pub fn require_auth(headers: &HeaderMap, state: &AppState) -> Result<(), ServerError> {
    let Some(expected) = &state.api_key else {
        return Ok(());
    };
    let provided = headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if provided == expected {
        Ok(())
    } else {
        Err(ServerError::Forbidden("invalid or missing API key".into()))
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .nest("/api/projects", routes::projects::projects_router())
        .nest("/api/tasks", routes::tasks::tasks_router())
        .nest("/api/projects/{project_id}/agent-configs", routes::agent_configs::agent_configs_router())
        .nest("/api/provider-configs", routes::agent_configs::provider_configs_router())
        .layer(DefaultBodyLimit::max(1024 * 100))
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}
