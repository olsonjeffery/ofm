use omprint::db;
use omprint::server;
use omprint::server::state::AppState;
use std::sync::{Arc, Mutex};

fn make_state() -> AppState {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    db::run_migrations(&conn).unwrap();
    let user_id = db::ensure_default_user(&conn).unwrap();
    AppState {
        db: Arc::new(Mutex::new(conn)),
        default_user_id: user_id,
    }
}

#[tokio::test]
async fn test_health_endpoint() {
    let app = server::router(make_state());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let url = format!("http://{}/health", addr);
    let client = reqwest::Client::new();
    let resp = client.get(&url).send().await.unwrap();

    assert_eq!(resp.status(), 200);
    assert_eq!(resp.text().await.unwrap(), "ok");
}
