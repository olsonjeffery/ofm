use omprint::db;
use rusqlite::Connection;
use uuid::Uuid;

fn setup_db() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    conn
}

fn uuid_text() -> String {
    Uuid::new_v4().to_string()
}

#[test]
fn test_all_migrations_apply() {
    let conn = setup_db();
    let count = db::run_migrations(&conn).unwrap();
    assert_eq!(
        count, 11,
        "All 11 DDL migrations should be applied on first run"
    );
}

#[test]
fn test_migrations_idempotent() {
    let conn = setup_db();
    let first = db::run_migrations(&conn).unwrap();
    let second = db::run_migrations(&conn).unwrap();
    assert!(first > 0);
    assert_eq!(second, 0, "Second run should apply 0 new migrations");
}

#[test]
fn test_insert_and_query_user() {
    let conn = setup_db();
    db::run_migrations(&conn).unwrap();

    let user_id = uuid_text();
    conn.execute(
        "INSERT INTO users (id, username) VALUES (?1, ?2)",
        [&user_id, "testuser"],
    )
    .unwrap();

    let (db_id, db_username): (String, String) = conn
        .query_row(
            "SELECT id, username FROM users WHERE id = ?1",
            [&user_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();

    assert_eq!(db_id, user_id);
    assert_eq!(db_username, "testuser");
}

#[test]
fn test_insert_and_query_project() {
    let conn = setup_db();
    db::run_migrations(&conn).unwrap();

    let user_id = uuid_text();
    conn.execute(
        "INSERT INTO users (id, username) VALUES (?1, ?2)",
        [&user_id, "projuser"],
    )
    .unwrap();

    let project_id = uuid_text();
    conn.execute(
        "INSERT INTO projects (id, user_id, name, repo_folder_path) VALUES (?1, ?2, ?3, ?4)",
        [&project_id, &user_id, "my-project", "/tmp/repo"],
    )
    .unwrap();

    let (db_id, db_name): (String, String) = conn
        .query_row(
            "SELECT id, name FROM projects WHERE id = ?1",
            [&project_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();

    assert_eq!(db_id, project_id);
    assert_eq!(db_name, "my-project");
}

#[test]
fn test_insert_and_query_task() {
    let conn = setup_db();
    db::run_migrations(&conn).unwrap();

    let user_id = uuid_text();
    conn.execute(
        "INSERT INTO users (id, username) VALUES (?1, ?2)",
        [&user_id, "taskuser"],
    )
    .unwrap();

    let project_id = uuid_text();
    conn.execute(
        "INSERT INTO projects (id, user_id, name, repo_folder_path) VALUES (?1, ?2, ?3, ?4)",
        [&project_id, &user_id, "p", "/tmp/r"],
    )
    .unwrap();

    let task_id = uuid_text();
    conn.execute(
        "INSERT INTO tasks (id, project_id, user_id, title) VALUES (?1, ?2, ?3, ?4)",
        [&task_id, &project_id, &user_id, "Test task"],
    )
    .unwrap();

    let (db_id, db_title, db_status, db_yolo): (String, String, String, i32) = conn
        .query_row(
            "SELECT id, title, status, yolo_mode FROM tasks WHERE id = ?1",
            [&task_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();

    assert_eq!(db_id, task_id);
    assert_eq!(db_title, "Test task");
    assert_eq!(db_status, "pending");
    assert_eq!(db_yolo, 0);
}

#[test]
fn test_insert_and_query_project_member() {
    let conn = setup_db();
    db::run_migrations(&conn).unwrap();

    let user_id = uuid_text();
    conn.execute(
        "INSERT INTO users (id, username) VALUES (?1, ?2)",
        [&user_id, "pmuser"],
    )
    .unwrap();

    let project_id = uuid_text();
    conn.execute(
        "INSERT INTO projects (id, user_id, name, repo_folder_path) VALUES (?1, ?2, ?3, ?4)",
        [&project_id, &user_id, "p", "/tmp/r"],
    )
    .unwrap();

    let pm_id = uuid_text();
    conn.execute(
        "INSERT INTO project_members (id, project_id, user_id) VALUES (?1, ?2, ?3)",
        [&pm_id, &project_id, &user_id],
    )
    .unwrap();

    let row_count: i32 = conn
        .query_row(
            "SELECT COUNT(*) FROM project_members WHERE project_id = ?1 AND user_id = ?2",
            [&project_id, &user_id],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(row_count, 1);
}

#[test]
fn test_unique_constraint_project_members() {
    let conn = setup_db();
    db::run_migrations(&conn).unwrap();

    let user_id = uuid_text();
    conn.execute(
        "INSERT INTO users (id, username) VALUES (?1, ?2)",
        [&user_id, "uniquser"],
    )
    .unwrap();

    let project_id = uuid_text();
    conn.execute(
        "INSERT INTO projects (id, user_id, name, repo_folder_path) VALUES (?1, ?2, ?3, ?4)",
        [&project_id, &user_id, "p", "/tmp/r"],
    )
    .unwrap();

    let pm_id = uuid_text();
    conn.execute(
        "INSERT INTO project_members (id, project_id, user_id) VALUES (?1, ?2, ?3)",
        [&pm_id, &project_id, &user_id],
    )
    .unwrap();

    let pm_id2 = uuid_text();
    let result = conn.execute(
        "INSERT INTO project_members (id, project_id, user_id) VALUES (?1, ?2, ?3)",
        [&pm_id2, &project_id, &user_id],
    );

    assert!(
        result.is_err(),
        "UNIQUE constraint should prevent duplicate project_members"
    );
}

#[test]
fn test_default_values() {
    let conn = setup_db();
    db::run_migrations(&conn).unwrap();

    let user_id = uuid_text();
    conn.execute(
        "INSERT INTO users (id, username) VALUES (?1, ?2)",
        [&user_id, "defaultuser"],
    )
    .unwrap();

    let (is_admin, is_technical, has_completed_onboarding): (i32, i32, i32) = conn
        .query_row(
            "SELECT is_admin, is_technical, has_completed_onboarding FROM users WHERE id = ?1",
            [&user_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();

    assert_eq!(is_admin, 0);
    assert_eq!(is_technical, 0);
    assert_eq!(has_completed_onboarding, 0);
}

#[test]
fn test_on_delete_cascade_project_members() {
    let conn = setup_db();
    db::run_migrations(&conn).unwrap();

    let user_id = uuid_text();
    conn.execute(
        "INSERT INTO users (id, username) VALUES (?1, ?2)",
        [&user_id, "cascadeuser"],
    )
    .unwrap();

    let project_id = uuid_text();
    conn.execute(
        "INSERT INTO projects (id, user_id, name, repo_folder_path) VALUES (?1, ?2, ?3, ?4)",
        [&project_id, &user_id, "p", "/tmp/r"],
    )
    .unwrap();

    let pm_id = uuid_text();
    conn.execute(
        "INSERT INTO project_members (id, project_id, user_id) VALUES (?1, ?2, ?3)",
        [&pm_id, &project_id, &user_id],
    )
    .unwrap();

    conn.execute("DELETE FROM projects WHERE id = ?1", [&project_id])
        .unwrap();

    let row_count: i32 = conn
        .query_row(
            "SELECT COUNT(*) FROM project_members WHERE id = ?1",
            [&pm_id],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(row_count, 0, "Project member should be cascade-deleted");
}

#[test]
fn test_conversation_foreign_key() {
    let conn = setup_db();
    db::run_migrations(&conn).unwrap();

    let user_id = uuid_text();
    conn.execute(
        "INSERT INTO users (id, username) VALUES (?1, ?2)",
        [&user_id, "convuser"],
    )
    .unwrap();

    let project_id = uuid_text();
    conn.execute(
        "INSERT INTO projects (id, user_id, name, repo_folder_path) VALUES (?1, ?2, ?3, ?4)",
        [&project_id, &user_id, "p", "/tmp/r"],
    )
    .unwrap();

    let task_id = uuid_text();
    conn.execute(
        "INSERT INTO tasks (id, project_id, user_id, title) VALUES (?1, ?2, ?3, ?4)",
        [&task_id, &project_id, &user_id, "conv task"],
    )
    .unwrap();

    let conv_id = uuid_text();
    conn.execute(
        "INSERT INTO conversations (id, task_id, model) VALUES (?1, ?2, ?3)",
        [&conv_id, &task_id, "gpt-4"],
    )
    .unwrap();

    let (db_id, db_model, db_effort): (String, String, String) = conn
        .query_row(
            "SELECT id, model, effort FROM conversations WHERE id = ?1",
            [&conv_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();

    assert_eq!(db_id, conv_id);
    assert_eq!(db_model, "gpt-4");
    assert_eq!(db_effort, "medium");
}

#[test]
fn test_app_settings() {
    let conn = setup_db();
    db::run_migrations(&conn).unwrap();

    conn.execute(
        "INSERT INTO app_settings (key, value) VALUES (?1, ?2)",
        ["theme", "dark"],
    )
    .unwrap();

    let (db_key, db_value): (String, String) = conn
        .query_row(
            "SELECT key, value FROM app_settings WHERE key = ?1",
            ["theme"],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();

    assert_eq!(db_key, "theme");
    assert_eq!(db_value, "dark");
}

#[test]
fn test_app_settings_upsert() {
    let conn = setup_db();
    db::run_migrations(&conn).unwrap();

    conn.execute(
        "INSERT INTO app_settings (key, value) VALUES (?1, ?2)",
        ["theme", "light"],
    )
    .unwrap();

    let result = conn.execute(
        "INSERT OR REPLACE INTO app_settings (key, value) VALUES (?1, ?2)",
        ["theme", "dark"],
    );

    assert!(result.is_ok(), "INSERT OR REPLACE should succeed");

    let db_value: String = conn
        .query_row(
            "SELECT value FROM app_settings WHERE key = ?1",
            ["theme"],
            |row| row.get(0),
        )
        .unwrap();

    assert_eq!(db_value, "dark");
}
