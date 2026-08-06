use ofm::auth::AuthLayer;
use ofm::config::OfmConfig;
use ofm::db;
use ofm::db::schema::AgentType;
use ofm::providers::LlmProvider;
use ofm::server;
use ofm::server::state::AppState;
use ofm::server::ws::bus::BroadcastBus;
use std::collections::HashMap;
use std::sync::Arc;
use tempfile::TempDir;
use tokio::sync::Mutex;
use uuid::Uuid;

async fn setup_app() -> (String, tokio::task::JoinHandle<()>, hiqlite::Client, Uuid) {
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
    db::ensure_static_prompts(&client).await.unwrap();
    let user_id = db::ensure_default_user(&client).await.unwrap();

    let auth_layer = AuthLayer::disabled(
        client.clone(),
        b"test".to_vec(),
        cookie::Key::generate(),
        user_id,
    );
    let state = AppState {
        cfg_port: 0,
        rauthy_port: None,

        db: client.clone(),
        default_user_id: user_id,
        footprint: tmp.path().to_str().unwrap().to_string(),
        archive_root: "storage/".into(),
        config_root: tmp.path().to_str().unwrap().to_string(),
        active_sessions: Arc::new(Mutex::new(HashMap::<String, Box<dyn LlmProvider>>::new())),
        oidc_provider: None,
        pkce_store: Arc::new(Mutex::new(HashMap::new())),
        cookie_key: cookie::Key::generate(),
        api_key_pepper: b"test_pepper".to_vec(),
        ws_bus: BroadcastBus::new(),
        config: OfmConfig::default(),
    };

    let app = server::router(state, auth_layer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = format!("http://{}", listener.local_addr().unwrap());
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (addr, handle, client, user_id)
}

fn client() -> reqwest::Client {
    reqwest::Client::new()
}

async fn create_snippet(addr: &str, title: &str, content: &str) -> serde_json::Value {
    let resp = client()
        .post(format!("{addr}/api/prompts"))
        .json(&serde_json::json!({
            "kind": "snippet",
            "title": title,
            "content": content,
            "tags": [],
            "is_shared": false,
            "children": [],
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    resp.json().await.unwrap()
}

#[tokio::test]
async fn test_prompts_crud_round_trip() {
    let (addr, _handle, _client, _uid) = setup_app().await;

    let created = create_snippet(&addr, "My Snippet", "Project: {{projectName}}").await;
    let id = created["id"].as_str().unwrap().to_string();

    let list = client()
        .get(format!("{addr}/api/prompts"))
        .send()
        .await
        .unwrap();
    assert_eq!(list.status(), 200);
    let library: serde_json::Value = list.json().await.unwrap();
    assert!(library
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["id"] == id && p["title"] == "My Snippet"));

    let detail = client()
        .get(format!("{addr}/api/prompts/{id}"))
        .send()
        .await
        .unwrap();
    assert_eq!(detail.status(), 200);
    let detail: serde_json::Value = detail.json().await.unwrap();
    assert_eq!(detail["prompt"]["title"], "My Snippet");

    let update = client()
        .put(format!("{addr}/api/prompts/{id}"))
        .json(&serde_json::json!({
            "title": "Renamed",
            "content": "Project: {{projectId}}",
            "tags": ["desktop-3d"],
            "is_shared": true,
            "children": [],
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(update.status(), 200);
    let updated: serde_json::Value = update.json().await.unwrap();
    assert_eq!(updated["title"], "Renamed");
    assert_eq!(updated["is_shared"], true);
    assert_eq!(updated["tags"][0], "desktop-3d");

    let del = client()
        .delete(format!("{addr}/api/prompts/{id}"))
        .send()
        .await
        .unwrap();
    assert_eq!(del.status(), 204);

    let after = client()
        .get(format!("{addr}/api/prompts/{id}"))
        .send()
        .await
        .unwrap();
    assert_eq!(after.status(), 404);
}

#[tokio::test]
async fn test_prompts_validate_endpoint() {
    let (addr, _handle, _client, _uid) = setup_app().await;

    let bad = client()
        .post(format!("{addr}/api/prompts/validate"))
        .json(&serde_json::json!({
            "content": "{{taskId}} {{bogus}}",
            "tags": ["Hello World", "desktop-3d"],
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(bad.status(), 200);
    let bad_body: serde_json::Value = bad.json().await.unwrap();
    assert_eq!(bad_body["valid"], false);
    assert_eq!(bad_body["unknownTokens"][0], "bogus");
    assert_eq!(bad_body["invalidTags"][0], "Hello World");

    let good = client()
        .post(format!("{addr}/api/prompts/validate"))
        .json(&serde_json::json!({
            "content": "{{taskId}} {{projectName}}",
            "tags": ["desktop-3d"],
        }))
        .send()
        .await
        .unwrap();
    let good_body: serde_json::Value = good.json().await.unwrap();
    assert_eq!(good_body["valid"], true);
    assert!(good_body["unknownTokens"].as_array().unwrap().is_empty());
    assert!(good_body["invalidTags"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn test_prompts_create_rejects_unknown_tokens_and_bad_tags() {
    let (addr, _handle, _client, _uid) = setup_app().await;
    let resp = client()
        .post(format!("{addr}/api/prompts"))
        .json(&serde_json::json!({
            "kind": "snippet",
            "title": "Bad",
            "content": "{{bogus}}",
            "tags": ["Hello World"],
            "is_shared": false,
            "children": [],
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn test_prompts_duplicate() {
    let (addr, _handle, _client, _uid) = setup_app().await;
    let created = create_snippet(&addr, "Original", "content {{taskId}}").await;
    let id = created["id"].as_str().unwrap().to_string();

    let dup = client()
        .post(format!("{addr}/api/prompts/{id}/duplicate"))
        .send()
        .await
        .unwrap();
    assert_eq!(dup.status(), 201);
    let copy: serde_json::Value = dup.json().await.unwrap();
    assert_eq!(copy["title"], "Original (copy)");
    assert_eq!(copy["content"], "content {{taskId}}");
    assert_ne!(copy["id"].as_str().unwrap(), id);
}

#[tokio::test]
async fn test_static_prompts_immutable_via_api() {
    let (addr, _handle, _client, _uid) = setup_app().await;
    let list = client()
        .get(format!("{addr}/api/prompts"))
        .send()
        .await
        .unwrap();
    let library: serde_json::Value = list.json().await.unwrap();
    let static_prompt = library
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["is_static"] == true)
        .unwrap();
    let id = static_prompt["id"].as_str().unwrap().to_string();

    let update = client()
        .put(format!("{addr}/api/prompts/{id}"))
        .json(&serde_json::json!({
            "title": "x",
            "content": "y",
            "tags": [],
            "is_shared": false,
            "children": [],
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(update.status(), 403);

    let del = client()
        .delete(format!("{addr}/api/prompts/{id}"))
        .send()
        .await
        .unwrap();
    assert_eq!(del.status(), 403);

    // Static prompts are still duplicable.
    let dup = client()
        .post(format!("{addr}/api/prompts/{id}/duplicate"))
        .send()
        .await
        .unwrap();
    assert_eq!(dup.status(), 201);
}

#[tokio::test]
async fn test_edit_other_users_prompt_forbidden() {
    let (addr, _handle, client_db, _uid) = setup_app().await;
    // A second user's prompt, not owned by the default (acting) user.
    let other_user = Uuid::new_v4();
    client_db
        .execute(
            "INSERT INTO users (id, username, is_admin, is_active, created_at) VALUES ($1, $2, 0, 1, '2024-01-01 00:00:00')",
            hiqlite::params!(other_user.to_string(), "other"),
        )
        .await
        .unwrap();
    let now = "2024-01-01 00:00:00";
    let prompt_id = Uuid::new_v4();
    client_db
        .execute(
            "INSERT INTO prompts (id, kind, title, content, owner_user_id, is_static, is_shared, created_at, updated_at) \
             VALUES ($1, 'snippet', 'Theirs', 'content', $2, 0, 0, $3, $3)",
            hiqlite::params!(prompt_id.to_string(), other_user.to_string(), now),
        )
        .await
        .unwrap();

    let update = client()
        .put(format!("{addr}/api/prompts/{prompt_id}"))
        .json(&serde_json::json!({
            "title": "Stolen",
            "content": "content",
            "tags": [],
            "is_shared": false,
            "children": [],
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(update.status(), 403);

    let del = client()
        .delete(format!("{addr}/api/prompts/{prompt_id}"))
        .send()
        .await
        .unwrap();
    assert_eq!(del.status(), 403);
}

#[tokio::test]
async fn test_assignments_upsert_list_delete() {
    let (addr, _handle, _client, _uid) = setup_app().await;
    let created = create_snippet(&addr, "Review prompt", "review content").await;
    let id = created["id"].as_str().unwrap().to_string();

    let assign = client()
        .post(format!("{addr}/api/prompts/{id}/assignments"))
        .json(&serde_json::json!({
            "agent_type": "review",
            "scope_type": "global",
            "project_id": null,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(assign.status(), 201);
    let assignment: serde_json::Value = assign.json().await.unwrap();
    let assignment_id = assignment["id"].as_str().unwrap().to_string();

    let list = client()
        .get(format!("{addr}/api/prompts/assignments"))
        .send()
        .await
        .unwrap();
    assert_eq!(list.status(), 200);
    let assignments: serde_json::Value = list.json().await.unwrap();
    assert_eq!(assignments.as_array().unwrap().len(), 1);

    let del = client()
        .delete(format!("{addr}/api/prompts/assignments/{assignment_id}"))
        .send()
        .await
        .unwrap();
    assert_eq!(del.status(), 204);

    let list = client()
        .get(format!("{addr}/api/prompts/assignments"))
        .send()
        .await
        .unwrap();
    let assignments: serde_json::Value = list.json().await.unwrap();
    assert!(assignments.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn test_plan_template_not_assignable() {
    let (addr, _handle, _client, _uid) = setup_app().await;
    let list = client()
        .get(format!("{addr}/api/prompts"))
        .send()
        .await
        .unwrap();
    let library: serde_json::Value = list.json().await.unwrap();
    let plan_template = library
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["static_key"] == "plan-template")
        .unwrap();
    let id = plan_template["id"].as_str().unwrap().to_string();

    let assign = client()
        .post(format!("{addr}/api/prompts/{id}/assignments"))
        .json(&serde_json::json!({
            "agent_type": "implementation",
            "scope_type": "global",
            "project_id": null,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(assign.status(), 400);
}

#[tokio::test]
async fn test_project_tags_create_update() {
    let (addr, _handle, _client, _uid) = setup_app().await;

    let create = client()
        .post(format!("{addr}/api/projects"))
        .json(&serde_json::json!({
            "name": "tagged-project",
            "repo_folder_path": "/tmp/tagged-repo",
            "tags": ["desktop-3d", "graphics"],
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(create.status(), 201);
    let created: serde_json::Value = create.json().await.unwrap();
    let project_id = created["id"].as_i64().unwrap();
    assert_eq!(created["tags"][0], "desktop-3d");
    assert_eq!(created["tags"][1], "graphics");

    let update = client()
        .put(format!("{addr}/api/projects/{project_id}"))
        .json(&serde_json::json!({
            "tags": ["web-app"]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(update.status(), 200);
    let updated: serde_json::Value = update.json().await.unwrap();
    assert_eq!(updated["tags"], serde_json::json!(["web-app"]));

    // Bad tag grammar is rejected server-side.
    let bad = client()
        .post(format!("{addr}/api/projects"))
        .json(&serde_json::json!({
            "name": "bad-tags",
            "repo_folder_path": "/tmp/bad-tags-repo",
            "tags": ["Hello World"],
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(bad.status(), 400);
}

#[tokio::test]
async fn test_project_scoped_assignment_and_resolution() {
    let (addr, _handle, client_db, _uid) = setup_app().await;

    let project_id = {
        let create = client()
            .post(format!("{addr}/api/projects"))
            .json(&serde_json::json!({
                "name": "scoped-project",
                "repo_folder_path": "/tmp/scoped-repo",
                "tags": ["desktop-3d"],
            }))
            .send()
            .await
            .unwrap();
        create.json::<serde_json::Value>().await.unwrap()["id"]
            .as_i64()
            .unwrap()
    };

    let global = create_snippet(&addr, "Global impl", "global {{taskId}}").await;
    let project_prompt = create_snippet(&addr, "Project impl", "project {{taskId}}").await;

    // Global assignment for implementation.
    let assign_global = client()
        .post(format!(
            "{addr}/api/prompts/{}/assignments",
            global["id"].as_str().unwrap()
        ))
        .json(&serde_json::json!({
            "agent_type": "implementation",
            "scope_type": "global",
            "project_id": null,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(assign_global.status(), 201);

    // Project-scoped assignment for the same agent type.
    let assign_project = client()
        .post(format!(
            "{addr}/api/prompts/{}/assignments",
            project_prompt["id"].as_str().unwrap()
        ))
        .json(&serde_json::json!({
            "agent_type": "implementation",
            "scope_type": "project",
            "project_id": project_id,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(assign_project.status(), 201);

    // The assignments list for the project shows both global and project rows.
    let list = client()
        .get(format!(
            "{addr}/api/prompts/assignments?project_id={project_id}"
        ))
        .send()
        .await
        .unwrap();
    let assignments: serde_json::Value = list.json().await.unwrap();
    assert_eq!(assignments.as_array().unwrap().len(), 2);

    // Resolution prefers the project-scoped prompt.
    let default_user = db::ensure_default_user(&client_db).await.unwrap();
    let resolved = ofm::services::prompts::resolve_prompt_for_agent(
        &client_db,
        &AgentType::Implementation,
        &default_user,
        project_id,
    )
    .await
    .unwrap()
    .expect("a designated prompt should resolve");
    assert_eq!(resolved.title, "Project impl");

    // The `{{tags}}` token resolves from the project in the orchestration path;
    // here we exercise the preview endpoint.
    let preview = client()
        .get(format!(
            "{addr}/api/prompts/{}/preview?project_id={project_id}",
            project_prompt["id"].as_str().unwrap()
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(preview.status(), 200);
}

#[tokio::test]
async fn test_webapp_prompts_pages_render() {
    let (addr, _handle, _client, _uid) = setup_app().await;
    let created = create_snippet(&addr, "Webapp Snippet", "Project: {{projectName}}").await;
    let id = created["id"].as_str().unwrap().to_string();

    let listing = client()
        .get(format!("{addr}/webapp/prompts"))
        .send()
        .await
        .unwrap();
    assert_eq!(listing.status(), 200);
    let body = listing.text().await.unwrap();
    assert!(body.contains("Prompt Library"));
    assert!(body.contains("Webapp Snippet"));
    assert!(body.contains("New Snippet"));

    let detail = client()
        .get(format!("{addr}/webapp/prompts/{id}"))
        .send()
        .await
        .unwrap();
    assert_eq!(detail.status(), 200);
    let body = detail.text().await.unwrap();
    assert!(body.contains("Prompt Builder"));
    assert!(body.contains("Webapp Snippet"));
    assert!(body.contains("Designate"));
}

#[tokio::test]
async fn test_composite_children_ordering_via_api() {
    let (addr, _handle, _client, _uid) = setup_app().await;
    let a = create_snippet(&addr, "A", "first").await;
    let b = create_snippet(&addr, "B", "second").await;

    let composite = client()
        .post(format!("{addr}/api/prompts"))
        .json(&serde_json::json!({
            "kind": "composite",
            "title": "Composite",
            "content": "",
            "tags": [],
            "is_shared": false,
            "children": [a["id"].as_str().unwrap(), b["id"].as_str().unwrap()],
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(composite.status(), 201);
    let composite: serde_json::Value = composite.json().await.unwrap();
    let composite_id = composite["id"].as_str().unwrap().to_string();

    let detail = client()
        .get(format!("{addr}/api/prompts/{composite_id}"))
        .send()
        .await
        .unwrap();
    let detail: serde_json::Value = detail.json().await.unwrap();
    let children = detail["children"].as_array().unwrap();
    assert_eq!(children[0]["title"], "A");
    assert_eq!(children[1]["title"], "B");
}
