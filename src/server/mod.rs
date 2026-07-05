pub mod error;
pub mod routes;
pub mod state;

use axum::{extract::DefaultBodyLimit, routing::get, Router};

use crate::auth::AuthLayer;
use crate::server::state::AppState;

pub fn router(state: AppState, auth_layer: AuthLayer) -> Router {
    let public = Router::new()
        .route("/health", get(health))
        .nest("/api/auth", routes::auth::auth_router());

    let protected = Router::new()
        .nest("/api/auth", routes::auth::auth_protected_router())
        .nest("/api/admin", routes::admin::admin_router())
        .nest("/api/projects", routes::projects::projects_router())
        .nest("/api/tasks", routes::tasks::tasks_router())
        .nest(
            "/api/projects/{project_id}/agent-configs",
            routes::agent_configs::agent_configs_router(),
        )
        .nest(
            "/api/provider-configs",
            routes::agent_configs::provider_configs_router(),
        )
        .layer(DefaultBodyLimit::max(1024 * 100))
        .layer(auth_layer);

    Router::new()
        .merge(public)
        .merge(protected)
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}
