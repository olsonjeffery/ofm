use ofm::db;
use tempfile::TempDir;
use uuid::Uuid;

async fn setup_db() -> (hiqlite::Client, TempDir) {
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
    (client, tmp)
}

fn uuid_text() -> String {
    Uuid::new_v4().to_string()
}

fn int64_id() -> i64 {
    use std::sync::atomic::{AtomicI64, Ordering};
    static NEXT_ID: AtomicI64 = AtomicI64::new(1);
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

#[tokio::test]
async fn test_all_migrations_apply() {
    let (client, _tmp) = setup_db().await;
    let count = db::run_migrations(&client).await.unwrap();
    assert_eq!(
        count, 39,
        "All 39 DDL migrations should be applied on first run"
    );
}

#[tokio::test]
async fn test_migrations_idempotent() {
    let (client, _tmp) = setup_db().await;
    let first = db::run_migrations(&client).await.unwrap();
    let second = db::run_migrations(&client).await.unwrap();
    assert!(first > 0);
    assert_eq!(second, 0, "Second run should apply 0 new migrations");
}

#[tokio::test]
async fn test_insert_and_query_user() {
    let (client, _tmp) = setup_db().await;
    db::run_migrations(&client).await.unwrap();

    let user_id = uuid_text();
    client
        .execute(
            "INSERT INTO users (id, username) VALUES ($1, $2)",
            hiqlite::params!(&user_id, "testuser"),
        )
        .await
        .unwrap();

    let mut rows = client
        .query_raw(
            "SELECT id, username FROM users WHERE id = $1",
            hiqlite::params!(&user_id),
        )
        .await
        .unwrap();
    let row = rows.first_mut().unwrap();
    let db_id: String = row.get("id");
    let db_username: String = row.get("username");

    assert_eq!(db_id, user_id);
    assert_eq!(db_username, "testuser");
}

#[tokio::test]
async fn test_insert_and_query_project() {
    let (client, _tmp) = setup_db().await;
    db::run_migrations(&client).await.unwrap();

    let user_id = uuid_text();
    client
        .execute(
            "INSERT INTO users (id, username) VALUES ($1, $2)",
            hiqlite::params!(&user_id, "projuser"),
        )
        .await
        .unwrap();

    let project_id = int64_id();
    client
        .execute(
            "INSERT INTO projects (id, user_id, name, repo_folder_path) VALUES ($1, $2, $3, $4)",
            hiqlite::params!(project_id, &user_id, "my-project", "/tmp/repo"),
        )
        .await
        .unwrap();

    let mut rows = client
        .query_raw(
            "SELECT id, name FROM projects WHERE id = $1",
            hiqlite::params!(project_id),
        )
        .await
        .unwrap();
    let row = rows.first_mut().unwrap();
    let db_id: i64 = row.get("id");
    let db_name: String = row.get("name");

    assert_eq!(db_id, project_id);
    assert_eq!(db_name, "my-project");
}

#[tokio::test]
async fn test_prompt_tables_and_project_tags_exist() {
    let (client, _tmp) = setup_db().await;
    db::run_migrations(&client).await.unwrap();

    // prompts table is usable end-to-end (insert + select).
    let user_id = uuid_text();
    client
        .execute(
            "INSERT INTO users (id, username) VALUES ($1, $2)",
            hiqlite::params!(&user_id, "promptuser"),
        )
        .await
        .unwrap();

    let prompt_id = uuid_text();
    client
        .execute(
            "INSERT INTO prompts (id, kind, title, content, owner_user_id, is_static, is_shared, created_at, updated_at) \
             VALUES ($1, 'snippet', 'My Snippet', 'content', $2, 0, 0, '2024-01-01 00:00:00', '2024-01-01 00:00:00')",
            hiqlite::params!(&prompt_id, &user_id),
        )
        .await
        .unwrap();

    let child_id = uuid_text();
    client
        .execute(
            "INSERT INTO prompts (id, kind, title, owner_user_id, created_at, updated_at) \
             VALUES ($1, 'composite', 'My Composite', $2, '2024-01-01 00:00:00', '2024-01-01 00:00:00')",
            hiqlite::params!(&child_id, &user_id),
        )
        .await
        .unwrap();

    client
        .execute(
            "INSERT INTO prompt_children (parent_id, child_id, position) VALUES ($1, $2, 0)",
            hiqlite::params!(&child_id, &prompt_id),
        )
        .await
        .unwrap();

    client
        .execute(
            "INSERT INTO prompt_assignments (id, prompt_id, agent_type, scope_type, project_id, created_at) \
             VALUES ($1, $2, 'review', 'global', NULL, '2024-01-01 00:00:00')",
            hiqlite::params!(uuid_text(), &prompt_id),
        )
        .await
        .unwrap();

    // projects.tags column exists and round-trips.
    let project_id = int64_id();
    client
        .execute(
            "INSERT INTO projects (id, user_id, name, repo_folder_path, tags) VALUES ($1, $2, 'p', '/tmp/r', '[\"desktop-3d\"]')",
            hiqlite::params!(project_id, &user_id),
        )
        .await
        .unwrap();
    let mut rows = client
        .query_raw(
            "SELECT tags FROM projects WHERE id = $1",
            hiqlite::params!(project_id),
        )
        .await
        .unwrap();
    let tags: String = rows.first_mut().unwrap().get("tags");
    assert_eq!(tags, "[\"desktop-3d\"]");
}

#[tokio::test]
async fn test_insert_and_query_task() {
    let (client, _tmp) = setup_db().await;
    db::run_migrations(&client).await.unwrap();

    let user_id = uuid_text();
    client
        .execute(
            "INSERT INTO users (id, username) VALUES ($1, $2)",
            hiqlite::params!(&user_id, "taskuser"),
        )
        .await
        .unwrap();

    let project_id = int64_id();
    client
        .execute(
            "INSERT INTO projects (id, user_id, name, repo_folder_path) VALUES ($1, $2, $3, $4)",
            hiqlite::params!(project_id, &user_id, "p", "/tmp/r"),
        )
        .await
        .unwrap();

    let task_id = int64_id();
    client
        .execute(
            "INSERT INTO tasks (id, project_id, user_id, title) VALUES ($1, $2, $3, $4)",
            hiqlite::params!(task_id, project_id, &user_id, "Test task"),
        )
        .await
        .unwrap();

    let mut rows = client
        .query_raw(
            "SELECT id, title, status, yolo_mode FROM tasks WHERE id = $1",
            hiqlite::params!(task_id),
        )
        .await
        .unwrap();
    let row = rows.first_mut().unwrap();
    let db_id: i64 = row.get("id");
    let db_title: String = row.get("title");
    let db_status: String = row.get("status");
    let db_yolo: i64 = row.get("yolo_mode");

    assert_eq!(db_id, task_id);
    assert_eq!(db_title, "Test task");
    assert_eq!(db_status, "pending");
    assert_eq!(db_yolo, 0);
}

#[tokio::test]
async fn test_insert_and_query_group() {
    let (client, _tmp) = setup_db().await;
    db::run_migrations(&client).await.unwrap();

    let user_id = uuid_text();
    client
        .execute(
            "INSERT INTO users (id, username) VALUES ($1, $2)",
            hiqlite::params!(&user_id, "groupowner"),
        )
        .await
        .unwrap();

    let group_id = uuid_text();
    client
        .execute(
            "INSERT INTO groups (id, name, is_org, is_oauth_scope, title, description, owner_id, created_by, created_at) \
             VALUES ($1, 'team-a', 0, 0, 'Team A', 'desc', $2, 'owner', '2024-01-01 00:00:00')",
            hiqlite::params!(&group_id, &user_id),
        )
        .await
        .unwrap();

    let mut rows = client
        .query_raw(
            "SELECT name, is_org, is_oauth_scope, title, description, owner_id FROM groups WHERE id = $1",
            hiqlite::params!(&group_id),
        )
        .await
        .unwrap();
    let row = rows.first_mut().unwrap();
    assert_eq!(row.get::<String>("name"), "team-a");
    assert_eq!(row.get::<i64>("is_org"), 0);
    assert_eq!(row.get::<i64>("is_oauth_scope"), 0);
    assert_eq!(row.get::<String>("title"), "Team A");
    assert_eq!(row.get::<String>("owner_id"), user_id);
}

#[tokio::test]
async fn test_insert_and_query_group_member() {
    let (client, _tmp) = setup_db().await;
    db::run_migrations(&client).await.unwrap();

    let owner_id = uuid_text();
    let member_id = uuid_text();
    client
        .execute(
            "INSERT INTO users (id, username) VALUES ($1, $2)",
            hiqlite::params!(&owner_id, "gmowner"),
        )
        .await
        .unwrap();
    client
        .execute(
            "INSERT INTO users (id, username) VALUES ($1, $2)",
            hiqlite::params!(&member_id, "gmmember"),
        )
        .await
        .unwrap();

    let group_id = uuid_text();
    client
        .execute(
            "INSERT INTO groups (id, name, owner_id) VALUES ($1, 'gm-group', $2)",
            hiqlite::params!(&group_id, &owner_id),
        )
        .await
        .unwrap();

    let gm_id = uuid_text();
    client
        .execute(
            "INSERT INTO group_members (id, group_id, user_id, member_group_id, level) VALUES ($1, $2, $3, NULL, 'contributor')",
            hiqlite::params!(&gm_id, &group_id, &member_id),
        )
        .await
        .unwrap();

    let mut rows = client
        .query_raw(
            "SELECT COUNT(*) as cnt FROM group_members WHERE group_id = $1 AND user_id = $2",
            hiqlite::params!(&group_id, &member_id),
        )
        .await
        .unwrap();
    let row_count: i64 = rows.first_mut().map(|r| r.get("cnt")).unwrap_or(0);

    assert_eq!(row_count, 1);

    // level default
    let mut rows = client
        .query_raw(
            "SELECT level FROM group_members WHERE id = $1",
            hiqlite::params!(&gm_id),
        )
        .await
        .unwrap();
    let level: String = rows.first_mut().unwrap().get("level");
    assert_eq!(level, "contributor");
}

#[tokio::test]
async fn test_unique_constraint_group_members() {
    let (client, _tmp) = setup_db().await;
    db::run_migrations(&client).await.unwrap();

    let owner_id = uuid_text();
    let member_id = uuid_text();
    client
        .execute(
            "INSERT INTO users (id, username) VALUES ($1, $2)",
            hiqlite::params!(&owner_id, "uniqowner"),
        )
        .await
        .unwrap();
    client
        .execute(
            "INSERT INTO users (id, username) VALUES ($1, $2)",
            hiqlite::params!(&member_id, "uniqmember"),
        )
        .await
        .unwrap();

    let group_id = uuid_text();
    client
        .execute(
            "INSERT INTO groups (id, name, owner_id) VALUES ($1, 'uniq-group', $2)",
            hiqlite::params!(&group_id, &owner_id),
        )
        .await
        .unwrap();

    let gm_id = uuid_text();
    client
        .execute(
            "INSERT INTO group_members (id, group_id, user_id, level) VALUES ($1, $2, $3, 'read-only')",
            hiqlite::params!(&gm_id, &group_id, &member_id),
        )
        .await
        .unwrap();

    let gm_id2 = uuid_text();
    let result = client
        .execute(
            "INSERT INTO group_members (id, group_id, user_id, level) VALUES ($1, $2, $3, 'admin')",
            hiqlite::params!(&gm_id2, &group_id, &member_id),
        )
        .await;

    assert!(
        result.is_err(),
        "UNIQUE constraint should prevent duplicate group_members"
    );
}

#[tokio::test]
async fn test_default_values() {
    let (client, _tmp) = setup_db().await;
    db::run_migrations(&client).await.unwrap();

    let user_id = uuid_text();
    client
        .execute(
            "INSERT INTO users (id, username) VALUES ($1, $2)",
            hiqlite::params!(&user_id, "defaultuser"),
        )
        .await
        .unwrap();

    let mut rows = client
        .query_raw(
            "SELECT is_admin, is_technical, has_completed_onboarding FROM users WHERE id = $1",
            hiqlite::params!(&user_id),
        )
        .await
        .unwrap();
    let row = rows.first_mut().unwrap();
    let is_admin: i64 = row.get("is_admin");
    let is_technical: i64 = row.get("is_technical");
    let has_completed_onboarding: i64 = row.get("has_completed_onboarding");

    assert_eq!(is_admin, 0);
    assert_eq!(is_technical, 0);
    assert_eq!(has_completed_onboarding, 0);
}

#[tokio::test]
async fn test_on_delete_cascade_group_members() {
    let (client, _tmp) = setup_db().await;
    db::run_migrations(&client).await.unwrap();

    let owner_id = uuid_text();
    let member_id = uuid_text();
    client
        .execute(
            "INSERT INTO users (id, username) VALUES ($1, $2)",
            hiqlite::params!(&owner_id, "cascadeowner"),
        )
        .await
        .unwrap();
    client
        .execute(
            "INSERT INTO users (id, username) VALUES ($1, $2)",
            hiqlite::params!(&member_id, "cascademember"),
        )
        .await
        .unwrap();

    let group_id = uuid_text();
    client
        .execute(
            "INSERT INTO groups (id, name, owner_id) VALUES ($1, 'cascade-group', $2)",
            hiqlite::params!(&group_id, &owner_id),
        )
        .await
        .unwrap();

    let gm_id = uuid_text();
    client
        .execute(
            "INSERT INTO group_members (id, group_id, user_id, level) VALUES ($1, $2, $3, 'read-only')",
            hiqlite::params!(&gm_id, &group_id, &member_id),
        )
        .await
        .unwrap();

    client
        .execute(
            "DELETE FROM groups WHERE id = $1",
            hiqlite::params!(&group_id),
        )
        .await
        .unwrap();

    let mut rows = client
        .query_raw(
            "SELECT COUNT(*) as cnt FROM group_members WHERE id = $1",
            hiqlite::params!(&gm_id),
        )
        .await
        .unwrap();
    let row_count: i64 = rows.first_mut().map(|r| r.get("cnt")).unwrap_or(0);

    assert_eq!(row_count, 0, "Group member should be cascade-deleted");
}

#[tokio::test]
async fn test_project_members_table_is_dropped() {
    let (client, _tmp) = setup_db().await;
    db::run_migrations(&client).await.unwrap();

    let result = client
        .execute(
            "INSERT INTO project_members (id, project_id, user_id) VALUES ($1, 1, $2)",
            hiqlite::params!(uuid_text(), uuid_text()),
        )
        .await;
    assert!(
        result.is_err(),
        "project_members table should have been dropped by the migrations"
    );

    // The groups schema is intact after the drop.
    let mut rows = client
        .query_raw("SELECT COUNT(*) as cnt FROM groups", hiqlite::params!())
        .await
        .unwrap();
    let row_count: i64 = rows.first_mut().map(|r| r.get("cnt")).unwrap_or(0);
    assert_eq!(row_count, 0);
}

#[tokio::test]
async fn test_users_scopes_column() {
    let (client, _tmp) = setup_db().await;
    db::run_migrations(&client).await.unwrap();

    let user_id = uuid_text();
    client
        .execute(
            "INSERT INTO users (id, username, scopes) VALUES ($1, $2, $3)",
            hiqlite::params!(&user_id, "scopeuser", "openid profile custom:scope"),
        )
        .await
        .unwrap();

    let mut rows = client
        .query_raw(
            "SELECT scopes FROM users WHERE id = $1",
            hiqlite::params!(&user_id),
        )
        .await
        .unwrap();
    let scopes: String = rows.first_mut().unwrap().get("scopes");
    assert_eq!(scopes, "openid profile custom:scope");
}

#[tokio::test]
async fn test_conversation_foreign_key() {
    let (client, _tmp) = setup_db().await;
    db::run_migrations(&client).await.unwrap();

    let user_id = uuid_text();
    client
        .execute(
            "INSERT INTO users (id, username) VALUES ($1, $2)",
            hiqlite::params!(&user_id, "convuser"),
        )
        .await
        .unwrap();

    let project_id = int64_id();
    client
        .execute(
            "INSERT INTO projects (id, user_id, name, repo_folder_path) VALUES ($1, $2, $3, $4)",
            hiqlite::params!(project_id, &user_id, "p", "/tmp/r"),
        )
        .await
        .unwrap();

    let task_id = int64_id();
    client
        .execute(
            "INSERT INTO tasks (id, project_id, user_id, title) VALUES ($1, $2, $3, $4)",
            hiqlite::params!(task_id, project_id, &user_id, "conv task"),
        )
        .await
        .unwrap();

    let conv_id = uuid_text();
    client
        .execute(
            "INSERT INTO conversations (id, task_id, model) VALUES ($1, $2, $3)",
            hiqlite::params!(&conv_id, task_id, "gpt-4"),
        )
        .await
        .unwrap();

    let mut rows = client
        .query_raw(
            "SELECT id, model, effort FROM conversations WHERE id = $1",
            hiqlite::params!(&conv_id),
        )
        .await
        .unwrap();
    let row = rows.first_mut().unwrap();
    let db_id: String = row.get("id");
    let db_model: String = row.get("model");
    let db_effort: String = row.get("effort");

    assert_eq!(db_id, conv_id);
    assert_eq!(db_model, "gpt-4");
    assert_eq!(db_effort, "medium");
}

#[tokio::test]
async fn test_app_settings() {
    let (client, _tmp) = setup_db().await;
    db::run_migrations(&client).await.unwrap();

    client
        .execute(
            "INSERT INTO app_settings (key, value) VALUES ($1, $2)",
            hiqlite::params!("theme", "dark"),
        )
        .await
        .unwrap();

    let mut rows = client
        .query_raw(
            "SELECT key, value FROM app_settings WHERE key = $1",
            hiqlite::params!("theme"),
        )
        .await
        .unwrap();
    let row = rows.first_mut().unwrap();
    let db_key: String = row.get("key");
    let db_value: String = row.get("value");

    assert_eq!(db_key, "theme");
    assert_eq!(db_value, "dark");
}

#[tokio::test]
async fn test_app_settings_upsert() {
    let (client, _tmp) = setup_db().await;
    db::run_migrations(&client).await.unwrap();

    client
        .execute(
            "INSERT INTO app_settings (key, value) VALUES ($1, $2)",
            hiqlite::params!("theme", "light"),
        )
        .await
        .unwrap();

    let result = client
        .execute(
            "INSERT OR REPLACE INTO app_settings (key, value) VALUES ($1, $2)",
            hiqlite::params!("theme", "dark"),
        )
        .await;

    assert!(result.is_ok(), "INSERT OR REPLACE should succeed");

    let mut rows = client
        .query_raw(
            "SELECT value FROM app_settings WHERE key = $1",
            hiqlite::params!("theme"),
        )
        .await
        .unwrap();
    let db_value: String = rows.first_mut().map(|r| r.get("value")).unwrap_or_default();

    assert_eq!(db_value, "dark");
}
