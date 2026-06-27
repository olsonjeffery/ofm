pub mod error;
pub mod routes;
pub mod state;

use crate::server::state::AppState;
use axum::{routing::get, Router};

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .nest("/api/projects", routes::projects::projects_router())
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}
