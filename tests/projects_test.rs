use omprint::db;
use omprint::server;
use omprint::server::state::AppState;
use tempfile::TempDir;

async fn setup_app() -> (String, tokio::task::JoinHandle<()>) {
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

    let app = server::router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = format!("http://{}", listener.local_addr().unwrap());
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (addr, handle)
}

fn client() -> reqwest::Client {
    reqwest::Client::new()
}

#[tokio::test]
async fn test_create_project() {
    let (addr, _handle) = setup_app().await;
    let resp = client()
        .post(format!("{}/api/projects", addr))
        .json(&serde_json::json!({
            "name": "test-project",
            "repo_folder_path": "/tmp/test-repo",
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 201);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["name"], "test-project");
    assert_eq!(body["repo_folder_path"], "/tmp/test-repo");
    assert!(body["id"].as_str().unwrap().len() > 0);
}

#[tokio::test]
async fn test_create_project_empty_name() {
    let (addr, _handle) = setup_app().await;
    let resp = client()
        .post(format!("{}/api/projects", addr))
        .json(&serde_json::json!({
            "name": "",
            "repo_folder_path": "/tmp/test-repo",
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "name is required");
}

#[tokio::test]
async fn test_create_project_empty_repo_folder_path() {
    let (addr, _handle) = setup_app().await;
    let resp = client()
        .post(format!("{}/api/projects", addr))
        .json(&serde_json::json!({
            "name": "test-project",
            "repo_folder_path": "",
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "repo_folder_path is required");
}

#[tokio::test]
async fn test_list_projects() {
    let (addr, _handle) = setup_app().await;

    client()
        .post(format!("{}/api/projects", addr))
        .json(&serde_json::json!({
            "name": "test-project",
            "repo_folder_path": "/tmp/test-repo",
        }))
        .send()
        .await
        .unwrap();

    let resp = client()
        .get(format!("{}/api/projects", addr))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body.as_array().unwrap().len() >= 1);
    assert_eq!(body[0]["name"], "test-project");
}

#[tokio::test]
async fn test_get_project() {
    let (addr, _handle) = setup_app().await;

    let create_resp = client()
        .post(format!("{}/api/projects", addr))
        .json(&serde_json::json!({
            "name": "test-project",
            "repo_folder_path": "/tmp/test-repo",
        }))
        .send()
        .await
        .unwrap();

    let created: serde_json::Value = create_resp.json().await.unwrap();
    let project_id = created["id"].as_str().unwrap().to_string();

    let resp = client()
        .get(format!("{}/api/projects/{}", addr, project_id))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["id"], project_id);
    assert_eq!(body["name"], "test-project");
}

#[tokio::test]
async fn test_get_project_not_found() {
    let (addr, _handle) = setup_app().await;
    let resp = client()
        .get(format!(
            "{}/api/projects/00000000-0000-0000-0000-000000000000",
            addr
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 404);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "Project not found");
}

#[tokio::test]
async fn test_update_project() {
    let (addr, _handle) = setup_app().await;

    let create_resp = client()
        .post(format!("{}/api/projects", addr))
        .json(&serde_json::json!({
            "name": "test-project",
            "repo_folder_path": "/tmp/test-repo",
        }))
        .send()
        .await
        .unwrap();

    let created: serde_json::Value = create_resp.json().await.unwrap();
    let project_id = created["id"].as_str().unwrap().to_string();

    let resp = client()
        .put(format!("{}/api/projects/{}", addr, project_id))
        .json(&serde_json::json!({
            "name": "updated-name",
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["name"], "updated-name");
}

#[tokio::test]
async fn test_update_project_not_found() {
    let (addr, _handle) = setup_app().await;
    let resp = client()
        .put(format!(
            "{}/api/projects/00000000-0000-0000-0000-000000000000",
            addr
        ))
        .json(&serde_json::json!({
            "name": "updated-name",
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn test_update_project_empty_name() {
    let (addr, _handle) = setup_app().await;

    let create_resp = client()
        .post(format!("{}/api/projects", addr))
        .json(&serde_json::json!({
            "name": "test-project",
            "repo_folder_path": "/tmp/test-repo",
        }))
        .send()
        .await
        .unwrap();

    let created: serde_json::Value = create_resp.json().await.unwrap();
    let project_id = created["id"].as_str().unwrap().to_string();

    let resp = client()
        .put(format!("{}/api/projects/{}", addr, project_id))
        .json(&serde_json::json!({
            "name": "",
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn test_delete_project() {
    let (addr, _handle) = setup_app().await;

    let create_resp = client()
        .post(format!("{}/api/projects", addr))
        .json(&serde_json::json!({
            "name": "test-project",
            "repo_folder_path": "/tmp/test-repo",
        }))
        .send()
        .await
        .unwrap();

    let created: serde_json::Value = create_resp.json().await.unwrap();
    let project_id = created["id"].as_str().unwrap().to_string();

    let resp = client()
        .delete(format!("{}/api/projects/{}", addr, project_id))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["success"].as_bool().unwrap());
}

#[tokio::test]
async fn test_delete_project_not_found() {
    let (addr, _handle) = setup_app().await;
    let resp = client()
        .delete(format!(
            "{}/api/projects/00000000-0000-0000-0000-000000000000",
            addr
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn test_delete_then_get_returns_404() {
    let (addr, _handle) = setup_app().await;

    let create_resp = client()
        .post(format!("{}/api/projects", addr))
        .json(&serde_json::json!({
            "name": "test-project",
            "repo_folder_path": "/tmp/test-repo",
        }))
        .send()
        .await
        .unwrap();

    let created: serde_json::Value = create_resp.json().await.unwrap();
    let project_id = created["id"].as_str().unwrap().to_string();

    client()
        .delete(format!("{}/api/projects/{}", addr, project_id))
        .send()
        .await
        .unwrap();

    let resp = client()
        .get(format!("{}/api/projects/{}", addr, project_id))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 404);
}
