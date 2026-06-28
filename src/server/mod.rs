pub mod error;
pub mod routes;
pub mod state;

use crate::server::state::AppState;
use axum::{extract::DefaultBodyLimit, routing::get, Router};

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .nest("/api/projects", routes::projects::projects_router())
        .nest("/api/tasks", routes::tasks::tasks_router())
        .layer(DefaultBodyLimit::max(1024 * 100))
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}
