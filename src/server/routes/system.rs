use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;

use crate::auth::AuthUser;
use crate::server::error::ServerError;
use crate::server::state::AppState;
use crate::services;

#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    pub resource: Option<String>,
    pub limit: Option<i64>,
}

pub fn system_router() -> Router<AppState> {
    Router::new()
        .route("/status", get(status_handler))
        .route("/history", get(history_handler))
}

/// `GET /api/system/status` — full latest report + `running_services` count.
async fn status_handler(
    _auth: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ServerError> {
    let report = services::system_health::latest_report(&state.db)
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;
    Ok(Json(services::system_health::render_json(&report)))
}

/// `GET /api/system/history?resource=&limit=` — rolling rows for agent /
/// mermaid consumption (limit capped at 1000).
async fn history_handler(
    _auth: AuthUser,
    State(state): State<AppState>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<serde_json::Value>, ServerError> {
    let limit = query
        .limit
        .unwrap_or(50)
        .clamp(1, services::system_health::HISTORY_LIMIT);
    let rows = services::system_health::history_report(&state.db, query.resource.as_deref(), limit)
        .await
        .map_err(|e| ServerError::Internal(e.to_string()))?;
    let entries: Vec<_> = rows
        .iter()
        .map(|e| {
            json!({
                "id": e.id,
                "category": e.category,
                "resource": e.resource,
                "status": e.status,
                "detail": e.detail,
                "metadata": serde_json::from_str::<serde_json::Value>(&e.metadata)
                    .unwrap_or_else(|_| json!({})),
                "created_at": e.created_at,
            })
        })
        .collect();
    Ok(Json(json!({ "entries": entries })))
}
