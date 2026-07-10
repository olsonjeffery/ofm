pub mod bus;
pub mod connection;
pub mod message;

use axum::extract::ws::WebSocketUpgrade;
use axum::extract::State;
use axum::response::IntoResponse;

use crate::auth::AuthUser;
use crate::server::state::AppState;

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    auth: AuthUser,
) -> impl IntoResponse {
    let bus = state.ws_bus.clone();
    let user_id = auth.user_id;
    let db = state.db.clone();

    ws.on_upgrade(move |socket| async move {
        connection::handle_socket(socket, bus, user_id, db).await;
    })
}
