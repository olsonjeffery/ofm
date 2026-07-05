pub mod app;
pub mod auth_pages;
pub mod islands;
pub mod pages;
pub mod shim;
pub mod styles;

use crate::server::state::AppState;
use axum::{extract::Query, response::Html, routing::get, Router};
use leptos::prelude::*;
use std::collections::HashMap;

pub fn webapp_routes() -> Router<AppState> {
    Router::new()
        .route("/webapp", get(shell_handler))
        .route("/webapp/islands/uptime", get(uptime_handler))
}

pub fn webapp_protected_routes() -> Router<AppState> {
    Router::new().route("/webapp/islands/infocard", get(infocard_handler))
}

async fn shell_handler() -> Html<String> {
    let html = leptos::view! { <app::ShellPage /> }.to_html();

    let home_html = leptos::view! { <pages::home::HomePage /> }.to_html();
    let html = html.replace("<main></main>", &format!("<main>{}</main>", home_html));

    Html(html)
}

async fn uptime_handler() -> Html<String> {
    Html(islands::uptime::render_uptime())
}

async fn infocard_handler(Query(params): Query<HashMap<String, String>>) -> Html<String> {
    let title = params.get("title").map(|s| s.as_str()).unwrap_or("Info");
    let body = params
        .get("body")
        .map(|s| s.as_str())
        .unwrap_or("No content.");
    Html(islands::infocard::render_infocard(title, body))
}
