use axum::extract::{Path, State};
use axum::middleware;
use axum::routing::{get, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::auth::{AuthRejection, AuthUser};
use crate::server::error::ServerError;
use crate::server::state::AppState;

pub fn admin_router() -> Router<AppState> {
    Router::new()
        .route("/users", get(list_users))
        .route("/users/{id}", put(update_user))
        .layer(middleware::from_fn(require_admin))
}

async fn require_admin(
    auth: AuthUser,
    request: axum::extract::Request,
    next: middleware::Next,
) -> Result<axum::response::Response, AuthRejection> {
    if !auth.is_admin {
        return Err(AuthRejection::Forbidden);
    }
    Ok(next.run(request).await)
}

async fn list_users(
    State(state): State<AppState>,
    _auth: AuthUser,
) -> Result<Json<serde_json::Value>, ServerError> {
    let users = crate::services::auth::list_users(&state.db).await?;
    let safe_users: Vec<serde_json::Value> = users
        .into_iter()
        .map(|u| {
            json!({
                "id": u.id,
                "username": u.username,
                "oidc_subject": u.oidc_subject,
                "is_admin": u.is_admin,
                "is_technical": u.is_technical,
                "is_active": u.is_active,
                "created_at": u.created_at,
                "last_login": u.last_login,
            })
        })
        .collect();
    Ok(Json(json!({ "users": safe_users })))
}

#[derive(Deserialize)]
struct UpdateUserBody {
    is_admin: Option<bool>,
    is_active: Option<bool>,
}

async fn update_user(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(target_id): Path<Uuid>,
    Json(body): Json<UpdateUserBody>,
) -> Result<Json<serde_json::Value>, ServerError> {
    let ctx = crate::services::auth::UpdateUserContext {
        user_id: auth.user_id,
    };
    let user = crate::services::auth::update_user(
        &state.db,
        target_id,
        body.is_admin,
        body.is_active,
        &ctx,
    )
    .await?;
    Ok(Json(json!({
        "id": user.id,
        "username": user.username,
        "is_admin": user.is_admin,
        "is_active": user.is_active,
    })))
}
