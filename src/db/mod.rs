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
            id INTEGER PRIMARY KEY AUTOINCREMENT,
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
            project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            UNIQUE(project_id, user_id)
        )",
    ),
    (
        "create_tasks",
        "CREATE TABLE IF NOT EXISTS tasks (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            user_id TEXT NOT NULL REFERENCES users(id),
            title TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'pending',
            workflow_blocked INTEGER NOT NULL DEFAULT 0,
            workflow_run_count INTEGER NOT NULL DEFAULT 0,
            yolo_mode INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT ''
        )",
    ),
    (
        "create_conversations",
        "CREATE TABLE IF NOT EXISTS conversations (
            id TEXT PRIMARY KEY,
            task_id INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
            provider_session_id TEXT,
            model TEXT NOT NULL,
            effort TEXT NOT NULL DEFAULT 'medium',
            created_at TEXT NOT NULL DEFAULT ''
        )",
    ),
    (
        "create_task_agent_runs",
        "CREATE TABLE IF NOT EXISTS task_agent_runs (
            id TEXT PRIMARY KEY,
            task_id INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
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
            project_key INTEGER NOT NULL,
            session_id TEXT NOT NULL,
            seq INTEGER NOT NULL,
            entry_json TEXT NOT NULL,
            PRIMARY KEY (project_key, session_id, seq)
        )",
    ),
    (
        "create_session_summaries",
        "CREATE TABLE IF NOT EXISTS session_summaries (
            project_key INTEGER NOT NULL,
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
            project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            task_id INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
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
            project_id INTEGER,
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
        "create_user_model_configs",
        "CREATE TABLE IF NOT EXISTS user_model_configs (
            id TEXT PRIMARY KEY,
            user_id TEXT NOT NULL REFERENCES users(id),
            name TEXT NOT NULL,
            config_body TEXT NOT NULL DEFAULT '',
            harness TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL DEFAULT '',
            updated_at TEXT NOT NULL DEFAULT ''
        )",
    ),
    (
        "idx_user_model_configs_user",
        "CREATE INDEX IF NOT EXISTS idx_user_model_configs_user ON user_model_configs(user_id)",
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
    (
        "sessions_add_token_version",
        "ALTER TABLE sessions ADD COLUMN token_version INTEGER NOT NULL DEFAULT 0",
    ),
    (
        "sessions_add_id_token",
        "ALTER TABLE sessions ADD COLUMN id_token TEXT",
    ),
    (
        "conversations_add_updated_at",
        "ALTER TABLE conversations ADD COLUMN updated_at TEXT NOT NULL DEFAULT ''",
    ),
    (
        "messages_add_timestamp",
        "ALTER TABLE messages ADD COLUMN timestamp TEXT NOT NULL DEFAULT '1970-01-01 00:00:00'",
    ),
    (
        "sessions_add_access_token",
        "ALTER TABLE sessions ADD COLUMN access_token TEXT NOT NULL DEFAULT ''",
    ),
    (
        "create_groups",
        "CREATE TABLE IF NOT EXISTS groups (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL UNIQUE,
            is_org INTEGER NOT NULL DEFAULT 0,
            is_oauth_scope INTEGER NOT NULL DEFAULT 0,
            title TEXT NOT NULL DEFAULT '',
            description TEXT NOT NULL DEFAULT '',
            owner_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            created_by TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL DEFAULT ''
        )",
    ),
    (
        "create_group_members",
        "CREATE TABLE IF NOT EXISTS group_members (
            id TEXT PRIMARY KEY,
            group_id TEXT NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
            user_id TEXT REFERENCES users(id) ON DELETE CASCADE,
            member_group_id TEXT REFERENCES groups(id) ON DELETE CASCADE,
            level TEXT NOT NULL DEFAULT 'read-only',
            created_at TEXT NOT NULL DEFAULT '',
            CHECK ( (user_id IS NOT NULL) OR (member_group_id IS NOT NULL) ),
            UNIQUE(group_id, user_id),
            UNIQUE(group_id, member_group_id)
        )",
    ),
    (
        "users_add_scopes",
        "ALTER TABLE users ADD COLUMN scopes TEXT NOT NULL DEFAULT ''",
    ),
    (
        "drop_project_members",
        "DROP TABLE IF EXISTS project_members",
    ),
    (
        "projects_add_tags",
        "ALTER TABLE projects ADD COLUMN tags TEXT NOT NULL DEFAULT '[]'",
    ),
    (
        "create_prompts",
        "CREATE TABLE IF NOT EXISTS prompts (
            id TEXT PRIMARY KEY,
            kind TEXT NOT NULL,                 -- 'snippet' | 'composite' | 'static'
            title TEXT NOT NULL,
            content TEXT NOT NULL DEFAULT '',
            tags TEXT NOT NULL DEFAULT '[]',    -- JSON array of dash-based-name strings
            owner_user_id TEXT,                 -- NULL for static prompts
            is_static INTEGER NOT NULL DEFAULT 0,
            is_shared INTEGER NOT NULL DEFAULT 0,
            static_key TEXT,                    -- stable seed key for static prompts
            created_at TEXT NOT NULL DEFAULT '',
            updated_at TEXT NOT NULL DEFAULT ''
        )",
    ),
    (
        "create_prompt_children",
        "CREATE TABLE IF NOT EXISTS prompt_children (
            parent_id TEXT NOT NULL REFERENCES prompts(id) ON DELETE CASCADE,
            child_id TEXT NOT NULL REFERENCES prompts(id) ON DELETE CASCADE,
            position INTEGER NOT NULL,
            PRIMARY KEY (parent_id, position)
        )",
    ),
    (
        "create_prompt_assignments",
        "CREATE TABLE IF NOT EXISTS prompt_assignments (
            id TEXT PRIMARY KEY,
            prompt_id TEXT NOT NULL REFERENCES prompts(id) ON DELETE CASCADE,
            agent_type TEXT NOT NULL,
            scope_type TEXT NOT NULL,           -- 'global' | 'project'
            project_id INTEGER,
            created_at TEXT NOT NULL DEFAULT ''
        )",
    ),
    (
        "idx_prompt_assignments_unique",
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_prompt_assignments_unique \
         ON prompt_assignments(agent_type, scope_type, COALESCE(project_id, -1))",
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

/// Idempotent bootstrap seed for the built-in `admins` group (created once per
/// footprint). Runs at startup alongside `ensure_default_user`:
///
/// - Creates the `groups` row named `"admins"` if absent.
/// - Backfills the initial `admin`-level membership from whichever user is
///   currently `is_admin=1` if the membership row is absent.
///
/// The group owner is the same ofm admin user; because the admin row may be
/// created *after* the first seed (first login), this function must be safe to
/// re-run on every startup.
pub async fn ensure_admins_group(client: &Client) -> Result<(), Box<dyn std::error::Error>> {
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

    // There is always at least one admin: `ensure_default_user` (which `main`
    // calls first) seeds a `default` row with `is_admin=1`. Prefer the first
    // admin row so the membership points at a real, current admin.
    let mut admin_rows = client
        .query_raw(
            "SELECT id, username FROM users WHERE is_admin = 1 ORDER BY created_at, id LIMIT 1",
            hiqlite::params!(),
        )
        .await?;
    let (admin_id, admin_username) = if let Some(row) = admin_rows.first_mut() {
        (row.get::<String>("id"), row.get::<String>("username"))
    } else {
        tracing::warn!("ensure_admins_group: no admin user found; skipping seed");
        return Ok(());
    };

    let group_id = {
        let mut rows = client
            .query_raw(
                "SELECT id FROM groups WHERE name = 'admins'",
                hiqlite::params!(),
            )
            .await?;
        if let Some(row) = rows.first_mut() {
            row.get::<String>("id")
        } else {
            let id = Uuid::new_v4().to_string();
            client
                .execute(
                    "INSERT INTO groups (id, name, is_org, is_oauth_scope, title, description, owner_id, created_by, created_at) \
                     VALUES ($1, 'admins', 0, 0, 'Admins', '', $2, $3, $4)",
                    hiqlite::params!(id.clone(), admin_id.clone(), admin_username.clone(), now.clone()),
                )
                .await?;
            id
        }
    };

    let mut member_rows = client
        .query_raw(
            "SELECT id FROM group_members WHERE group_id = $1 AND user_id = $2 AND level = 'admin'",
            hiqlite::params!(group_id.clone(), admin_id.clone()),
        )
        .await?;
    if member_rows.first_mut().is_none() {
        client
            .execute(
                "INSERT INTO group_members (id, group_id, user_id, member_group_id, level, created_at) \
                 VALUES ($1, $2, $3, NULL, 'admin', $4)",
                hiqlite::params!(Uuid::new_v4().to_string(), group_id, admin_id, now),
            )
            .await?;
    }

    Ok(())
}

/// Idempotent seed of the 6 built-in `templates/*.md` prompts as
/// `kind='static'` rows owned by no user (`is_static=1`). Static prompts are
/// immutable and visible to every user in the library; users may duplicate them
/// into their own editable snippets. Seeding is keyed on a stable `static_key`
/// so re-runs never insert duplicates. Called at startup after
/// `ensure_admins_group` and from test helpers.
pub async fn ensure_static_prompts(client: &Client) -> Result<usize, Box<dyn std::error::Error>> {
    const STATIC_PROMPTS: &[(&str, &str, &str)] = &[
        (
            "planification",
            "Planification",
            include_str!("../../templates/planification.md"),
        ),
        (
            "plan-template",
            "Plan Template",
            include_str!("../../templates/plan-template.md"),
        ),
        (
            "implementation",
            "Implementation",
            include_str!("../../templates/implementation.md"),
        ),
        (
            "review",
            "Review",
            include_str!("../../templates/review.md"),
        ),
        (
            "refinement",
            "Refinement",
            include_str!("../../templates/refinement.md"),
        ),
        ("pr", "Pull Request", include_str!("../../templates/pr.md")),
    ];

    let mut inserted = 0;
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    for (static_key, title, content) in STATIC_PROMPTS {
        let rows = client
            .execute(
                "INSERT OR IGNORE INTO prompts \
                 (id, kind, title, content, owner_user_id, is_static, is_shared, static_key, created_at, updated_at) \
                 VALUES ($1, 'static', $2, $3, NULL, 1, 1, $4, $5, $5)",
                hiqlite::params!(
                    Uuid::new_v4().to_string(),
                    *title,
                    *content,
                    *static_key,
                    &now
                ),
            )
            .await?;
        inserted += rows;
    }
    Ok(inserted)
}

/// All OIDC subjects currently bound to user rows. `main` calls this right
/// after a rauthy re-bootstrap wipes rauthy's data volume, so the subjects that
/// are about to be orphaned can be recorded for the auth service's one-shot
/// re-link authorization.
pub async fn oidc_subjects(client: &Client) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut rows = client
        .query_raw(
            "SELECT oidc_subject FROM users WHERE oidc_subject IS NOT NULL AND oidc_subject != ''",
            hiqlite::params!(),
        )
        .await?;
    let mut subjects = Vec::new();
    for row in rows.iter_mut() {
        if let Some(sub) = row.get::<Option<String>>("oidc_subject") {
            subjects.push(sub);
        }
    }
    Ok(subjects)
}
