pub mod error;
pub mod proxy;
pub mod routes;
pub mod state;
pub mod ws;

use axum::{extract::DefaultBodyLimit, response::Redirect, routing::get, Router};

use crate::auth::AuthLayer;
use crate::server::proxy::rauthy_proxy_router;
use crate::server::state::AppState;
use crate::webapp;
use tower_http::services::ServeDir;

pub fn router(state: AppState, auth_layer: AuthLayer) -> Router {
    let mut public = Router::new()
        .route("/health", get(health))
        .route("/", get(|| async { Redirect::permanent("/webapp") }))
        .route("/webapp/", get(|| async { Redirect::permanent("/webapp") }))
        .nest("/api/auth", routes::auth::auth_router());

    // `/auth` is public (the browser hits the rauthy login page without an
    // OFM session) and only exists when rauthy is hosted locally. `state` is
    // moved into `with_state` below, so read `rauthy_port` first.
    if let Some(rauthy_port) = state.rauthy_port {
        public = public.nest_service(
            "/auth",
            rauthy_proxy_router(
                rauthy_port,
                &state.config.pub_url,
                state.config.rauthy_trusted_proxies.as_deref(),
            ),
        );
    }

    // Public webapp routes (no auth)
    let webapp_public = Router::new()
        .merge(webapp::webapp_routes())
        .nest_service("/webapp/assets", ServeDir::new("assets"));

    // Protected webapp routes (session cookie auth required, redirects to login on failure)
    let webapp_auth_layer = if auth_layer.enabled {
        webapp::auth::WebappAuthLayer::new(state.db.clone(), state.cookie_key.clone())
    } else {
        webapp::auth::WebappAuthLayer::disabled(
            state.db.clone(),
            state.cookie_key.clone(),
            state.default_user_id,
        )
    };
    let webapp_protected = webapp::webapp_protected_routes().layer(webapp_auth_layer);

    let protected = Router::new()
        .nest("/api/auth", routes::auth::auth_protected_router())
        .nest("/api/admin", routes::admin::admin_router())
        .nest("/api/groups", routes::groups::groups_router())
        .nest("/api/projects", routes::projects::projects_router())
        .nest("/api/prompts", routes::prompts::prompts_router())
        .nest("/api/tasks", routes::tasks::tasks_router())
        .nest(
            "/api/projects/{project_id}/agent-configs",
            routes::agent_configs::agent_configs_router(),
        )
        .nest(
            "/api/provider-configs",
            routes::agent_configs::provider_configs_router(),
        )
        .nest("/api/settings", routes::settings::settings_router())
        .nest("/api/system", routes::system::system_router())
        .route("/ws", get(ws::ws_handler))
        .layer(DefaultBodyLimit::max(1024 * 100))
        .layer(auth_layer);

    Router::new()
        .merge(public)
        .merge(webapp_public)
        .merge(webapp_protected)
        .merge(protected)
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}
