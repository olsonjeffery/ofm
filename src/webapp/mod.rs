pub mod app;
pub mod auth;
pub mod auth_pages;
pub mod components;
pub mod islands;
pub mod pages;
pub mod shim;
pub mod styles;

use std::collections::HashMap;

use axum::extract::{Query, State};
use axum::response::Html;
use axum::routing::get;
use axum::Router;
use axum_extra::extract::cookie::PrivateCookieJar;
use leptos::prelude::*;
use uuid::Uuid;

use crate::server::state::AppState;

pub fn webapp_routes() -> Router<AppState> {
    Router::new()
        .route("/webapp/login", get(login_handler))
        .route("/webapp/callback", get(callback_handler))
}

pub fn webapp_protected_routes() -> Router<AppState> {
    Router::new()
        .route("/webapp", get(shell_handler))
        .route("/webapp/islands/uptime", get(uptime_handler))
        .route("/webapp/islands/infocard", get(infocard_handler))
}

fn render_shell(page_html: &str) -> String {
    let shell = leptos::view! { <app::ShellPage /> }.to_html();
    shell.replace("<main></main>", &format!("<main>{page_html}</main>"))
}

async fn login_handler() -> Html<String> {
    let login_html = pages::login::render_login_page();
    Html(render_shell(&login_html))
}

async fn callback_handler(
    State(state): State<AppState>,
    jar: PrivateCookieJar,
    Query(_params): Query<HashMap<String, String>>,
) -> Html<String> {
    let redirect_to_login = || {
        Html(render_shell(
            r#"<script>window.location.href='/webapp/login';</script>"#,
        ))
    };

    let session_id = match crate::server::routes::auth::parse_session_cookie(&jar) {
        Ok(id) => id,
        Err(_) => return redirect_to_login(),
    };

    let user_id = match resolve_user_id_from_session(&state.db, session_id).await {
        Some(id) => id,
        None => return redirect_to_login(),
    };

    let access_token = match &state.oidc_provider {
        Some(oidc) => {
            match crate::services::auth::refresh_access_token(&state.db, oidc, session_id).await {
                Ok(token) => token,
                Err(_) => String::new(),
            }
        }
        None => String::new(),
    };

    let user_json = match crate::services::auth::current_user(&state.db, user_id).await {
        Ok(user) => serde_json::to_string(&user).unwrap_or_default(),
        Err(_) => return redirect_to_login(),
    };

    let onboarding_html = pages::onboarding::render_onboarding_form();
    let onboarding_json = serde_json::to_string(&onboarding_html).unwrap_or_default();

    let page_html = leptos::view! {
        <pages::callback::CallbackPage access_token user_json onboarding_html=onboarding_json />
    }
    .to_html();
    Html(render_shell(&page_html))
}

async fn resolve_user_id_from_session(db: &hiqlite::Client, session_id: Uuid) -> Option<Uuid> {
    let mut rows = db
        .query_raw(
            "SELECT * FROM sessions WHERE id = $1",
            hiqlite::params!(session_id.to_string()),
        )
        .await
        .ok()?;
    let session = rows.first_mut()?;
    let session_db = crate::db::schema::SessionDb::from(&mut *session);

    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    if session_db.expires_at < now {
        return None;
    }

    Some(session_db.user_id)
}

async fn shell_handler() -> Html<String> {
    let home_html = leptos::view! { <pages::home::HomePage /> }.to_html();
    Html(render_shell(&home_html))
}

async fn uptime_handler() -> Html<String> {
    Html(islands::uptime::render_uptime())
}

async fn infocard_handler(Query(params): Query<HashMap<String, String>>) -> Html<String> {
    let title = params.get("title").map(|s| s.as_str()).unwrap_or("Info");
    let body = params.get("body").map(|s| s.as_str()).unwrap_or("No content.");
    Html(islands::infocard::render_infocard(title, body))
}
