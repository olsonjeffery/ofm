pub mod schema;

use hiqlite::Client;
use uuid::Uuid;

const MIGRATIONS: &[(&str, &str)] = &[
    (
        "create_users",
        "CREATE TABLE IF NOT EXISTS users (
            id TEXT PRIMARY KEY,
            username TEXT NOT NULL UNIQUE,
            oidc_subject TEXT UNIQUE,
            is_admin INTEGER NOT NULL DEFAULT 0,
            is_technical INTEGER NOT NULL DEFAULT 0,
            has_completed_onboarding INTEGER NOT NULL DEFAULT 0,
            git_name TEXT,
            git_email TEXT,
            api_key_hash TEXT,
            api_key_last_used_at TEXT,
            token_version INTEGER NOT NULL DEFAULT 0
        )",
    ),
    (
        "create_projects",
        "CREATE TABLE IF NOT EXISTS projects (
            id TEXT PRIMARY KEY,
            user_id TEXT NOT NULL REFERENCES users(id),
            name TEXT NOT NULL,
            repo_folder_path TEXT NOT NULL,
            subproject_path TEXT,
            created_at TEXT NOT NULL DEFAULT ''
        )",
    ),
    (
        "create_project_members",
        "CREATE TABLE IF NOT EXISTS project_members (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            UNIQUE(project_id, user_id)
        )",
    ),
    (
        "create_tasks",
        "CREATE TABLE IF NOT EXISTS tasks (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            user_id TEXT NOT NULL REFERENCES users(id),
            title TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'pending',
            workflow_complete INTEGER NOT NULL DEFAULT 0,
            workflow_blocked INTEGER NOT NULL DEFAULT 0,
            workflow_run_count INTEGER NOT NULL DEFAULT 0,
            planification_complete INTEGER NOT NULL DEFAULT 0,
            pr_agent_complete INTEGER NOT NULL DEFAULT 0,
            refinement_complete INTEGER NOT NULL DEFAULT 0,
            yolo_mode INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT ''
        )",
    ),
    (
        "create_conversations",
        "CREATE TABLE IF NOT EXISTS conversations (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
            omp_session_id TEXT,
            model TEXT NOT NULL,
            effort TEXT NOT NULL DEFAULT 'medium',
            created_at TEXT NOT NULL DEFAULT ''
        )",
    ),
    (
        "create_task_agent_runs",
        "CREATE TABLE IF NOT EXISTS task_agent_runs (
            id TEXT PRIMARY KEY,
            task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
            agent_type TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'pending',
            conversation_id TEXT REFERENCES conversations(id) ON DELETE SET NULL,
            created_at TEXT NOT NULL DEFAULT '',
            completed_at TEXT
        )",
    ),
    (
        "create_messages",
        "CREATE TABLE IF NOT EXISTS messages (
            project_key TEXT NOT NULL,
            session_id TEXT NOT NULL,
            seq INTEGER NOT NULL,
            entry_json TEXT NOT NULL,
            PRIMARY KEY (project_key, session_id, seq)
        )",
    ),
    (
        "create_session_summaries",
        "CREATE TABLE IF NOT EXISTS session_summaries (
            project_key TEXT NOT NULL,
            session_id TEXT NOT NULL,
            mtime TEXT NOT NULL,
            summary_json TEXT NOT NULL,
            PRIMARY KEY (project_key, session_id)
        )",
    ),
    (
        "create_app_settings",
        "CREATE TABLE IF NOT EXISTS app_settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        )",
    ),
    (
        "create_user_agent_model_settings",
        "CREATE TABLE IF NOT EXISTS user_agent_model_settings (
            user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
            settings_json TEXT NOT NULL
        )",
    ),
    (
        "unique_project_repo_path",
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_projects_repo_folder_path ON projects(repo_folder_path)",
    ),
    (
        "create_worktrees",
        "CREATE TABLE IF NOT EXISTS worktrees (
            id TEXT PRIMARY KEY,
            project_uuid TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            task_uuid TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
            project_id INTEGER NOT NULL,
            task_id INTEGER NOT NULL,
            worktree_path TEXT NOT NULL,
            branch TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT ''
        )",
    ),
    (
        "worktrees_add_repo_path",
        "ALTER TABLE worktrees ADD COLUMN repo_path TEXT NOT NULL DEFAULT ''",
    ),
    (
        "idx_task_agent_runs_one_running",
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_task_agent_runs_one_running ON task_agent_runs(task_id) WHERE status = 'running'",
    ),
    (
        "conversations_add_name",
        "ALTER TABLE conversations ADD COLUMN name TEXT",
    ),
    (
        "create_agent_harness_configs",
        "CREATE TABLE IF NOT EXISTS agent_harness_configs (
            id TEXT PRIMARY KEY,
            agent_type TEXT NOT NULL,
            harness TEXT NOT NULL,
            provider_config_ref TEXT NOT NULL,
            scope_type TEXT NOT NULL DEFAULT 'global',
            user_id TEXT,
            project_id TEXT,
            model TEXT,
            effort TEXT,
            created_at TEXT NOT NULL DEFAULT '',
            updated_at TEXT NOT NULL DEFAULT ''
        )",
    ),
    (
        "idx_agent_harness_configs_unique",
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_harness_configs_unique \
         ON agent_harness_configs(agent_type, scope_type, COALESCE(user_id, ''), COALESCE(project_id, ''))",
    ),
    (
        "add_user_columns",
        "ALTER TABLE users ADD COLUMN is_active INTEGER NOT NULL DEFAULT 1",
    ),
    (
        "add_user_created_at",
        "ALTER TABLE users ADD COLUMN created_at TEXT NOT NULL DEFAULT ''",
    ),
    (
        "add_user_last_login",
        "ALTER TABLE users ADD COLUMN last_login TEXT",
    ),
    (
        "create_sessions_table",
        "CREATE TABLE IF NOT EXISTS sessions (
            id TEXT PRIMARY KEY,
            user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            refresh_token TEXT NOT NULL,
            expires_at TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT ''
        )",
    ),
];

pub async fn run_migrations(client: &Client) -> Result<usize, Box<dyn std::error::Error>> {
    client
        .execute(
            "CREATE TABLE IF NOT EXISTS _migrations (
            name TEXT PRIMARY KEY,
            applied_at TEXT NOT NULL DEFAULT ''
        )",
            hiqlite::params!(),
        )
        .await?;

    let mut rows = client
        .query_raw(
            "SELECT name FROM _migrations ORDER BY name",
            hiqlite::params!(),
        )
        .await?;
    let applied: Vec<String> = rows
        .iter_mut()
        .map(|row| row.get::<String>("name"))
        .collect();

    let mut count = 0;
    for (name, sql) in MIGRATIONS {
        if applied.iter().any(|a| a == name) {
            continue;
        }
        client.batch(*sql).await?;
        client
            .execute(
                "INSERT INTO _migrations (name) VALUES ($1)",
                hiqlite::params!(*name),
            )
            .await?;
        count += 1;
    }

    Ok(count)
}

pub async fn ensure_default_user(client: &Client) -> Result<Uuid, Box<dyn std::error::Error>> {
    let mut rows = client
        .query_raw(
            "SELECT id FROM users WHERE username = 'default'",
            hiqlite::params!(),
        )
        .await?;
    if let Some(row) = rows.first_mut() {
        let id_str: String = row.get("id");
        return Ok(Uuid::parse_str(&id_str)?);
    }
    let id = Uuid::new_v4();
    client
        .execute(
            "INSERT INTO users (id, username, is_admin, is_technical, is_active) VALUES ($1, 'default', 1, 1, 1)",
            hiqlite::params!(id.to_string()),
        )
        .await?;
    Ok(id)
}
