use omprint::db;
use omprint::server;
use omprint::server::state::AppState;
use tempfile::TempDir;

async fn make_state() -> (AppState, TempDir) {
    let tmp = TempDir::new().unwrap();
    let config = hiqlite::NodeConfig {
        node_id: 1,
        nodes: vec![hiqlite::Node {
            id: 1,
            addr_raft: "127.0.0.1:0".into(),
            addr_api: "127.0.0.1:0".into(),
        }],
        data_dir: tmp.path().to_str().unwrap().to_string().into(),
        secret_raft: "test-raft-secret-123".into(),
        secret_api: "test-api-secret-123".into(),
        ..Default::default()
    };
    let client = hiqlite::start_node(config).await.unwrap();
    client.wait_until_healthy_db().await;
    db::run_migrations(&client).await.unwrap();
    let user_id = db::ensure_default_user(&client).await.unwrap();
    let state = AppState {
        db: client,
        default_user_id: user_id,
        archive_root: "storage/".into(),
    };
    (state, tmp)
}

#[tokio::test]
async fn test_health_endpoint() {
    let (state, _tmp) = make_state().await;
    let app = server::router(state);
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
