use ofm::auth::AuthLayer;
use ofm::db;
use ofm::providers::LlmProvider;
use ofm::server;
use ofm::server::state::AppState;
use ofm::server::ws::bus::BroadcastBus;

use hiqlite::Client;
use std::collections::HashMap;
use std::sync::Arc;
use tempfile::TempDir;
use tokio::sync::Mutex;

struct TestApp {
    addr: String,
    _handle: tokio::task::JoinHandle<()>,
    db: Client,
    _project_id: i64,
    task_id: i64,
}

async fn setup_app() -> TestApp {
    let db_dir = TempDir::new().unwrap();
    let config = hiqlite::NodeConfig {
        node_id: 1,
        nodes: vec![hiqlite::Node {
            id: 1,
            addr_raft: "127.0.0.1:0".into(),
            addr_api: "127.0.0.1:0".into(),
        }],
        data_dir: db_dir.path().to_str().unwrap().to_string().into(),
        secret_raft: "test-raft-secret-123".into(),
        secret_api: "test-api-secret-123".into(),
        ..Default::default()
    };
    let client = hiqlite::start_node(config).await.unwrap();
    client.wait_until_healthy_db().await;
    db::run_migrations(&client).await.unwrap();
    let user_id = db::ensure_default_user(&client).await.unwrap();

    let project_id = ofm::services::projects::create_project(
        &client,
        &user_id,
        "test-project",
        "/tmp/test-repo",
        None,
    )
    .await
    .unwrap()
    .id;

    let task_id =
        ofm::services::tasks::create_task(&client, project_id, &user_id, "test task", "pending")
            .await
            .unwrap()
            .id;

    let auth_layer = AuthLayer::disabled(
        client.clone(),
        b"test".to_vec(),
        cookie::Key::generate(),
        user_id,
    );
    let state = AppState {
        cfg_port: 0,

        db: client.clone(),
        default_user_id: user_id,
        footprint: db_dir.path().to_str().unwrap().to_string(),
        archive_root: "storage/".into(),
        config_root: db_dir.path().to_str().unwrap().to_string(),
        active_sessions: Arc::new(Mutex::new(HashMap::<String, Box<dyn LlmProvider>>::new())),
        oidc_provider: None,
        pkce_store: Arc::new(Mutex::new(HashMap::new())),
        cookie_key: cookie::Key::generate(),
        api_key_pepper: b"test_pepper".to_vec(),
        ws_bus: BroadcastBus::new(),
    };

    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = format!("http://{}", listener.local_addr().unwrap());
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    TestApp {
        addr,
        _handle: handle,
        db: client,
        _project_id: project_id,
        task_id,
    }
}

fn client() -> reqwest::Client {
    reqwest::Client::new()
}

async fn assert_task_flags(
    db: &Client,
    task_id: i64,
    plan: bool,
    workflow: bool,
    blocked: bool,
    pr: bool,
) {
    let task = ofm::services::tasks::get_task(db, task_id).await.unwrap();
    assert_eq!(task.planification_complete, plan, "planification_complete");
    assert_eq!(task.workflow_complete, workflow, "workflow_complete");
    assert_eq!(task.workflow_blocked, blocked, "workflow_blocked");
    assert_eq!(task.pr_agent_complete, pr, "pr_agent_complete");
}

#[tokio::test]
async fn test_complete_plan() {
    let app = setup_app().await;
    let resp = client()
        .post(format!(
            "{}/api/tasks/{}/complete-plan",
            app.addr, app.task_id
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert_task_flags(&app.db, app.task_id, true, false, false, false).await;
}

#[tokio::test]
async fn test_complete_workflow() {
    let app = setup_app().await;
    let resp = client()
        .post(format!(
            "{}/api/tasks/{}/complete-workflow",
            app.addr, app.task_id
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert_task_flags(&app.db, app.task_id, false, true, false, false).await;
}

#[tokio::test]
async fn test_block_workflow() {
    let app = setup_app().await;
    let resp = client()
        .post(format!(
            "{}/api/tasks/{}/block-workflow",
            app.addr, app.task_id
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert_task_flags(&app.db, app.task_id, false, false, true, false).await;
}

#[tokio::test]
async fn test_complete_pr() {
    let app = setup_app().await;
    let resp = client()
        .post(format!(
            "{}/api/tasks/{}/complete-pr",
            app.addr, app.task_id
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert_task_flags(&app.db, app.task_id, false, false, false, true).await;
}

#[tokio::test]
async fn test_flags_are_independent() {
    let app = setup_app().await;
    // Set planification_complete and pr_agent_complete
    client()
        .post(format!(
            "{}/api/tasks/{}/complete-plan",
            app.addr, app.task_id
        ))
        .send()
        .await
        .unwrap();
    client()
        .post(format!(
            "{}/api/tasks/{}/complete-pr",
            app.addr, app.task_id
        ))
        .send()
        .await
        .unwrap();
    // Verify only the two targeted flags changed
    assert_task_flags(&app.db, app.task_id, true, false, false, true).await;
}

#[tokio::test]
async fn test_unknown_task_returns_404() {
    let app = setup_app().await;
    let fake_id: i64 = 99999;
    let resp = client()
        .post(format!("{}/api/tasks/{}/complete-plan", app.addr, fake_id))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn test_unknown_task_for_all_endpoints_return_404() {
    let app = setup_app().await;
    let fake_id: i64 = 99999;
    for endpoint in &[
        "complete-plan",
        "complete-workflow",
        "block-workflow",
        "complete-pr",
    ] {
        let resp = client()
            .post(format!("{}/api/tasks/{}/{}", app.addr, fake_id, endpoint))
            .send()
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            404,
            "endpoint /{} should return 404",
            endpoint
        );
    }
}

#[tokio::test]
async fn test_cli_complete_plan_exits_zero_and_flips_flag() {
    let app = setup_app().await;
    let task_id = app.task_id.to_string();

    let binary = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().and_then(|p| p.parent()).map(|p| p.join("ofm")))
        .expect("could not locate ofm binary");

    let output = tokio::process::Command::new(&binary)
        .args(["agent", "complete-plan", &task_id])
        .env("OFM_URL", &app.addr)
        .output()
        .await
        .unwrap();

    assert!(
        output.status.success(),
        "CLI exited with: {:?}",
        output.status
    );
    assert_task_flags(&app.db, app.task_id, true, false, false, false).await;
}

#[tokio::test]
async fn test_cli_all_commands_exit_zero() {
    let app = setup_app().await;
    let task_id = app.task_id.to_string();

    let binary = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().and_then(|p| p.parent()).map(|p| p.join("ofm")))
        .expect("could not locate ofm binary");

    for (action, plan, workflow, blocked, pr) in &[
        ("complete-plan", true, false, false, false),
        ("complete-workflow", true, true, false, false),
        ("block-workflow", true, true, true, false),
        ("complete-pr", true, true, true, true),
    ] {
        let output = tokio::process::Command::new(&binary)
            .args(["agent", action, &task_id])
            .env("OFM_URL", &app.addr)
            .output()
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "agent {} failed: {:?}",
            action,
            output.status
        );
        assert_task_flags(&app.db, app.task_id, *plan, *workflow, *blocked, *pr).await;
    }
}
