pub mod error;
pub mod routes;
pub mod state;

use axum::{routing::get, Router};
use crate::server::state::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .nest("/api/projects", routes::projects::projects_router())
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}
