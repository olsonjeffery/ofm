use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use axum_extra::extract::cookie::{Cookie, PrivateCookieJar, SameSite};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::server::error::ServerError;
use crate::server::state::AppState;
use crate::services::auth::urlencoding;

pub fn auth_router() -> Router<AppState> {
    Router::new()
        .route("/login", get(login))
        .route("/callback", get(callback))
        .route("/refresh", post(refresh))
        .route("/logout", post(logout))
}

pub fn auth_protected_router() -> Router<AppState> {
    Router::new()
        .route("/me", get(me))
        .route("/onboarding", patch(onboarding_handler))
        .route(
            "/api-key",
            post(generate_api_key_handler).delete(revoke_api_key_handler),
        )
}

async fn login(State(state): State<AppState>) -> Result<Json<serde_json::Value>, ServerError> {
    let oidc = state
        .oidc_provider
        .as_ref()
        .ok_or_else(|| ServerError::BadRequest("OIDC not configured".into()))?;

    let auth_url = crate::services::auth::initiate_login(oidc, &state.pkce_store).await?;
    Ok(Json(json!({ "authorization_url": auth_url })))
}

#[derive(Deserialize)]
struct CallbackQuery {
    code: String,
    state: String,
}

async fn callback(
    State(state): State<AppState>,
    jar: PrivateCookieJar,
    Query(query): Query<CallbackQuery>,
) -> Result<(PrivateCookieJar, impl IntoResponse), ServerError> {
    let oidc = state
        .oidc_provider
        .as_ref()
        .ok_or_else(|| ServerError::BadRequest("OIDC not configured".into()))?;

    let result = crate::services::auth::handle_callback(
        &state.db,
        oidc,
        &state.pkce_store,
        query.code,
        query.state,
    )
    .await?;

    let jar = jar.add(
        Cookie::build(("omprint_session", result.session_id.to_string()))
            .http_only(true)
            .same_site(SameSite::Lax)
            .path("/")
            .build(),
    );

    let location = if !result.has_completed_onboarding {
        "/webapp/callback".to_string()
    } else {
        "/webapp/".to_string()
    };
    Ok((jar, (StatusCode::FOUND, [("Location", location)])))
}

async fn refresh(
    State(state): State<AppState>,
    jar: PrivateCookieJar,
) -> Result<Json<serde_json::Value>, ServerError> {
    let oidc = state
        .oidc_provider
        .as_ref()
        .ok_or_else(|| ServerError::BadRequest("OIDC not configured".into()))?;

    let session_id = parse_session_cookie(&jar)?;

    let access_token =
        crate::services::auth::refresh_access_token(&state.db, oidc, session_id).await?;

    Ok(Json(json!({ "access_token": access_token })))
}

async fn logout(
    State(state): State<AppState>,
    jar: PrivateCookieJar,
) -> Result<(PrivateCookieJar, Json<serde_json::Value>), ServerError> {
    let mut end_session_url: Option<String> = None;

    if let Some(cookie) = jar.get("omprint_session") {
        if let Ok(sid) = Uuid::parse_str(cookie.value()) {
            let id_token = crate::services::auth::find_session(&state.db, sid)
                .await
                .ok()
                .flatten()
                .and_then(|s| s.id_token);

            crate::services::auth::logout(&state.db, &state.oidc_provider, sid).await?;

            if let Some(oidc) = &state.oidc_provider {
                if let Some(ees) = &oidc.end_session_endpoint {
                    let post_logout_uri = format!("http://127.0.0.1:{}/webapp", state.cfg_port);
                    let mut url = format!(
                        "{}?client_id={}&post_logout_redirect_uri={}",
                        ees,
                        urlencoding(&oidc.client_id),
                        urlencoding(&post_logout_uri),
                    );
                    if let Some(ref token) = id_token {
                        url.push_str(&format!("&id_token_hint={}", urlencoding(token)));
                    }
                    end_session_url = Some(url);
                }
            }
        }
    }

    let jar = jar.remove(Cookie::from("omprint_session"));

    Ok((jar, Json(json!({ "redirect_url": end_session_url }))))
}

async fn me(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<serde_json::Value>, ServerError> {
    let user = crate::services::auth::current_user(&state.db, auth.user_id).await?;

    Ok(Json(json!({
        "id": user.id,
        "username": user.username,
        "oidc_subject": user.oidc_subject,
        "is_admin": user.is_admin,
        "is_technical": user.is_technical,
        "has_completed_onboarding": user.has_completed_onboarding,
        "is_active": user.is_active,
        "created_at": user.created_at,
        "last_login": user.last_login,
    })))
}

async fn generate_api_key_handler(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<serde_json::Value>, ServerError> {
    let api_key =
        crate::services::auth::generate_api_key(&state.db, auth.user_id, &state.api_key_pepper)
            .await?;
    Ok(Json(json!({ "api_key": api_key })))
}

#[derive(Deserialize)]
struct OnboardingRequest {
    git_name: String,
    git_email: String,
    is_technical: bool,
}

async fn onboarding_handler(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<OnboardingRequest>,
) -> Result<Json<serde_json::Value>, ServerError> {
    let user = crate::services::auth::complete_onboarding(
        &state.db,
        auth.user_id,
        body.git_name,
        body.git_email,
        body.is_technical,
    )
    .await?;
    Ok(Json(json!({
        "success": true,
        "user": {
            "id": user.id,
            "username": user.username,
            "git_name": user.git_name,
            "git_email": user.git_email,
            "is_technical": user.is_technical,
            "has_completed_onboarding": user.has_completed_onboarding,
        }
    })))
}

async fn revoke_api_key_handler(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<serde_json::Value>, ServerError> {
    crate::services::auth::revoke_api_key(&state.db, auth.user_id).await?;
    Ok(Json(json!({ "success": true })))
}

pub(crate) fn parse_session_cookie(jar: &PrivateCookieJar) -> Result<Uuid, ServerError> {
    let cookie = jar
        .get("omprint_session")
        .ok_or_else(|| ServerError::BadRequest("no session cookie".into()))?;
    Uuid::parse_str(cookie.value())
        .map_err(|_| ServerError::BadRequest("invalid session cookie".into()))
}
