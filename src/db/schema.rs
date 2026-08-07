use chrono::NaiveDateTime;
use hiqlite::Row;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaskStatus {
    Pending,
    InProgress,
    InReview,
    Completed,
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::InProgress => write!(f, "in_progress"),
            Self::InReview => write!(f, "in_review"),
            Self::Completed => write!(f, "completed"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentType {
    Planification,
    Implementation,
    Refinement,
    Review,
    Pr,
    Yolo,
    ConversationTitle,
}

impl std::fmt::Display for AgentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Planification => write!(f, "planification"),
            Self::Implementation => write!(f, "implementation"),
            Self::Refinement => write!(f, "refinement"),
            Self::Review => write!(f, "review"),
            Self::Pr => write!(f, "pr"),
            Self::Yolo => write!(f, "yolo"),
            Self::ConversationTitle => write!(f, "conversation_title"),
        }
    }
}

impl std::str::FromStr for AgentType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "planification" => Ok(Self::Planification),
            "implementation" => Ok(Self::Implementation),
            "refinement" => Ok(Self::Refinement),
            "review" => Ok(Self::Review),
            "pr" => Ok(Self::Pr),
            "yolo" => Ok(Self::Yolo),
            "conversation_title" => Ok(Self::ConversationTitle),
            _ => Err(format!("invalid agent type: '{s}'")),
        }
    }
}

impl AgentType {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Planification => "Planification",
            Self::Implementation => "Implementation",
            Self::Refinement => "Refinement",
            Self::Review => "Review",
            Self::Pr => "PR",
            Self::Yolo => "Yolo",
            Self::ConversationTitle => "Conversation Title",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            Self::Planification => "file-document-outline",
            Self::Implementation => "code-tags",
            Self::Refinement => "creation-outline",
            Self::Review => "checkbox-marked-circle-outline",
            Self::Pr => "source-branch-plus",
            Self::Yolo => "rocket",
            Self::ConversationTitle => "message-text-outline",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Blocked,
}

impl std::fmt::Display for RunStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Running => write!(f, "running"),
            Self::Completed => write!(f, "completed"),
            Self::Failed => write!(f, "failed"),
            Self::Blocked => write!(f, "blocked"),
        }
    }
}

impl std::str::FromStr for RunStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "blocked" => Ok(Self::Blocked),
            _ => Err(format!("invalid run status: '{s}'")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: i64,
    pub user_id: Uuid,
    pub name: String,
    pub repo_folder_path: String,
    pub subproject_path: Option<String>,
    /// Dash-based-name tags (JSON array column). Rendered as pills in the UI and
    /// substituted into the `{{tags}}` prompt token.
    pub tags: Vec<String>,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: i64,
    pub project_id: i64,
    pub user_id: Uuid,
    pub title: String,
    pub status: String,
    pub workflow_blocked: bool,
    pub workflow_run_count: i32,
    pub yolo_mode: bool,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub id: Uuid,
    pub task_id: i64,
    pub provider_session_id: Option<String>,
    pub model: String,
    pub effort: String,
    pub name: Option<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationWithRun {
    pub conversation: Conversation,
    pub run: Option<TaskAgentRun>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveAgent {
    pub agent_type: AgentType,
    pub project_id: i64,
    pub project_title: String,
    pub task_id: i64,
    pub task_title: String,
    pub conversation_id: Uuid,
    pub conversation_name: Option<String>,
}

/// A task reference for the global agent-status feed (open questions, blocked
/// tasks) with just enough context to build a deep link in the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStatusSummary {
    pub project_id: i64,
    pub project_title: String,
    pub task_id: i64,
    pub task_title: String,
}

/// Aggregate, user-scoped view of agent activity for the navbar dropdown.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalAgentStatus {
    pub agents: Vec<ActiveAgent>,
    pub questions: Vec<TaskStatusSummary>,
    pub blocked: Vec<TaskStatusSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScopeType {
    Global,
    User,
    Project,
    UserProject,
}

impl std::fmt::Display for ScopeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Global => write!(f, "global"),
            Self::User => write!(f, "user"),
            Self::Project => write!(f, "project"),
            Self::UserProject => write!(f, "user_project"),
        }
    }
}

impl std::str::FromStr for ScopeType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "global" => Ok(Self::Global),
            "user" => Ok(Self::User),
            "project" => Ok(Self::Project),
            "user_project" => Ok(Self::UserProject),
            _ => Err(format!("invalid scope type: '{s}'")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentHarnessConfig {
    pub id: Uuid,
    pub agent_type: AgentType,
    pub harness: String,
    pub provider_config_ref: String,
    pub scope_type: ScopeType,
    pub user_id: Option<Uuid>,
    pub project_id: Option<i64>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskAgentRun {
    pub id: Uuid,
    pub task_id: i64,
    pub agent_type: AgentType,
    pub status: RunStatus,
    pub conversation_id: Option<Uuid>,
    pub created_at: NaiveDateTime,
    pub completed_at: Option<NaiveDateTime>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub project_key: i64,
    pub session_id: String,
    pub seq: i32,
    pub entry_json: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Worktree {
    pub id: Uuid,
    pub project_id: i64,
    pub task_id: i64,
    pub worktree_path: String,
    pub repo_path: String,
    pub branch: String,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub project_key: i64,
    pub session_id: String,
    pub mtime: NaiveDateTime,
    pub summary_json: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub oidc_subject: Option<String>,
    pub is_admin: bool,
    pub is_technical: bool,
    pub has_completed_onboarding: bool,
    pub git_name: Option<String>,
    pub git_email: Option<String>,
    pub api_key_hash: Option<String>,
    pub api_key_last_used_at: Option<String>,
    pub is_active: bool,
    pub token_version: i32,
    pub created_at: String,
    pub last_login: Option<String>,
    /// Space-delimited OAuth scopes granted to the user at login (from the
    /// token response, access-token `scope` claim, and/or userinfo echo).
    /// Used to evaluate membership of groups with `is_oauth_scope` set.
    pub scopes: String,
}

/// Fixed membership-level enum for `group_members.level`. Stored as a TEXT
/// column and validated in the service layer (SQLite has no native enums).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum GroupLevel {
    ReadOnly,
    Contributor,
    Maintainer,
    Admin,
}

impl GroupLevel {
    pub const LEVELS: [&'static str; 4] = ["read-only", "contributor", "maintainer", "admin"];
}

impl std::fmt::Display for GroupLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ReadOnly => write!(f, "read-only"),
            Self::Contributor => write!(f, "contributor"),
            Self::Maintainer => write!(f, "maintainer"),
            Self::Admin => write!(f, "admin"),
        }
    }
}

impl std::str::FromStr for GroupLevel {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "read-only" => Ok(Self::ReadOnly),
            "contributor" => Ok(Self::Contributor),
            "maintainer" => Ok(Self::Maintainer),
            "admin" => Ok(Self::Admin),
            _ => Err(format!(
                "invalid group level '{s}': must be one of {:?}",
                Self::LEVELS
            )),
        }
    }
}

/// A User Group / Organization definition (`groups` table).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    pub id: Uuid,
    pub name: String,
    pub is_org: bool,
    pub is_oauth_scope: bool,
    pub title: String,
    pub description: String,
    pub owner_id: Uuid,
    /// preferred_name snapshot of the creating user.
    pub created_by: String,
    pub created_at: NaiveDateTime,
}

/// Polymorphic membership row: references either a `user_id` or a
/// `member_group_id` (groups-of-groups roll-up).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupMember {
    pub id: Uuid,
    pub group_id: Uuid,
    pub user_id: Option<Uuid>,
    pub member_group_id: Option<Uuid>,
    pub level: String,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionDb {
    pub id: Uuid,
    pub user_id: Uuid,
    pub token_version: i32,
    pub refresh_token: String,
    pub id_token: Option<String>,
    pub access_token: String,
    pub expires_at: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSetting {
    pub key: String,
    pub value: String,
}

/// One row of the rolling `system_health_entry` log. The table is append-only:
/// every refresh inserts fresh rows and prunes to the newest [`crate::services::system_health::MAX_ROWS_PER_PRUNE`].
/// Latest-state-per-resource is `ORDER BY id DESC` deduped (see
/// `services::system_health::latest_report`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemHealthEntryDb {
    pub id: i64,
    pub category: String,
    pub resource: String,
    pub status: String,
    pub detail: String,
    /// Heterogeneous JSON: `version`, `path`, `install_method`, `pid`,
    /// `ram_kb`, `last_interaction`, ...
    pub metadata: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserAgentModelSetting {
    pub user_id: Uuid,
    pub settings_json: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserModelConfig {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub config_body: String,
    pub harness: String,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

// hiqlite Row conversions

fn parse_naive_datetime(s: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f"))
        .unwrap_or_default()
}

impl From<&mut Row<'_>> for Project {
    fn from(row: &mut Row<'_>) -> Self {
        Self {
            id: row.get::<i64>("id"),
            user_id: row
                .get::<String>("user_id")
                .parse()
                .expect("invalid UUID in database"),
            name: row.get("name"),
            repo_folder_path: row.get("repo_folder_path"),
            subproject_path: row.get("subproject_path"),
            tags: serde_json::from_str(&row.get::<String>("tags")).unwrap_or_default(),
            created_at: parse_naive_datetime(&row.get::<String>("created_at")),
        }
    }
}

/// Prompt kind: a bare snippet, a composite (an ordered collection of other
/// prompts), or a static built-in template (immutable, no owner).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptKind {
    Snippet,
    Composite,
    Static,
}

impl std::fmt::Display for PromptKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Snippet => write!(f, "snippet"),
            Self::Composite => write!(f, "composite"),
            Self::Static => write!(f, "static"),
        }
    }
}

impl std::str::FromStr for PromptKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "snippet" => Ok(Self::Snippet),
            "composite" => Ok(Self::Composite),
            "static" => Ok(Self::Static),
            _ => Err(format!("invalid prompt kind: '{s}'")),
        }
    }
}

impl PromptKind {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Snippet => "snippet",
            Self::Composite => "composite",
            Self::Static => "static",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prompt {
    pub id: Uuid,
    pub kind: PromptKind,
    pub title: String,
    pub content: String,
    /// Dash-based-name tags (JSON array column).
    pub tags: Vec<String>,
    /// NULL for static prompts.
    pub owner_user_id: Option<Uuid>,
    pub is_static: bool,
    pub is_shared: bool,
    /// Stable seed key for static prompts (e.g. `"planification"`).
    pub static_key: Option<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptChild {
    pub parent_id: Uuid,
    pub child_id: Uuid,
    pub position: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptAssignment {
    pub id: Uuid,
    pub prompt_id: Uuid,
    pub agent_type: AgentType,
    pub scope_type: ScopeType,
    pub project_id: Option<i64>,
    pub created_at: NaiveDateTime,
}

impl From<&mut Row<'_>> for Task {
    fn from(row: &mut Row<'_>) -> Self {
        Self {
            id: row.get::<i64>("id"),
            project_id: row.get::<i64>("project_id"),
            user_id: row
                .get::<String>("user_id")
                .parse()
                .expect("invalid UUID in database"),
            title: row.get("title"),
            status: row.get("status"),
            workflow_blocked: row.get::<i64>("workflow_blocked") != 0,
            workflow_run_count: row.get::<i64>("workflow_run_count") as i32,
            yolo_mode: row.get::<i64>("yolo_mode") != 0,
            created_at: parse_naive_datetime(&row.get::<String>("created_at")),
        }
    }
}

impl From<&mut Row<'_>> for Worktree {
    fn from(row: &mut Row<'_>) -> Self {
        Self {
            id: row
                .get::<String>("id")
                .parse()
                .expect("invalid UUID in database"),
            project_id: row.get::<i64>("project_id"),
            task_id: row.get::<i64>("task_id"),
            worktree_path: row.get("worktree_path"),
            repo_path: row.get("repo_path"),
            branch: row.get("branch"),
            created_at: parse_naive_datetime(&row.get::<String>("created_at")),
        }
    }
}

impl From<&mut Row<'_>> for Message {
    fn from(row: &mut Row<'_>) -> Self {
        Self {
            project_key: row.get::<i64>("project_key"),
            session_id: row.get("session_id"),
            seq: row.get::<i64>("seq") as i32,
            entry_json: serde_json::from_str(&row.get::<String>("entry_json")).unwrap_or_default(),
        }
    }
}

impl From<&mut Row<'_>> for SessionSummary {
    fn from(row: &mut Row<'_>) -> Self {
        Self {
            project_key: row.get::<i64>("project_key"),
            session_id: row.get("session_id"),
            mtime: parse_naive_datetime(&row.get::<String>("mtime")),
            summary_json: serde_json::from_str(&row.get::<String>("summary_json"))
                .unwrap_or_default(),
        }
    }
}

impl From<&mut Row<'_>> for Conversation {
    fn from(row: &mut Row<'_>) -> Self {
        Self {
            id: row
                .get::<String>("id")
                .parse()
                .expect("invalid UUID in database"),
            task_id: row.get::<i64>("task_id"),
            provider_session_id: row.get("provider_session_id"),
            model: row.get("model"),
            effort: row.get("effort"),
            name: row.get("name"),
            created_at: parse_naive_datetime(&row.get::<String>("created_at")),
            updated_at: parse_naive_datetime(&row.get::<String>("updated_at")),
        }
    }
}

impl From<&mut Row<'_>> for AgentHarnessConfig {
    fn from(row: &mut Row<'_>) -> Self {
        let scope_type_str: String = row.get("scope_type");
        let scope_type = scope_type_str.parse().unwrap_or(ScopeType::Global);
        let agent_type_str: String = row.get("agent_type");
        let agent_type = agent_type_str.parse().unwrap_or(AgentType::Implementation);
        Self {
            id: row
                .get::<String>("id")
                .parse()
                .expect("invalid UUID in database"),
            agent_type,
            harness: row.get("harness"),
            provider_config_ref: row.get("provider_config_ref"),
            scope_type,
            user_id: row
                .get::<Option<String>>("user_id")
                .map(|s| Uuid::parse_str(&s).expect("invalid UUID in database")),
            project_id: row.get::<Option<i64>>("project_id"),
            model: row.get("model"),
            effort: row.get("effort"),
            created_at: parse_naive_datetime(&row.get::<String>("created_at")),
            updated_at: parse_naive_datetime(&row.get::<String>("updated_at")),
        }
    }
}

impl From<&mut Row<'_>> for TaskAgentRun {
    fn from(row: &mut Row<'_>) -> Self {
        let agent_type_str: String = row.get("agent_type");
        let agent_type = agent_type_str.parse().unwrap_or(AgentType::Implementation);
        let status_str: String = row.get("status");
        let status = status_str.parse().unwrap_or(RunStatus::Pending);
        Self {
            id: row
                .get::<String>("id")
                .parse()
                .expect("invalid UUID in database"),
            task_id: row.get::<i64>("task_id"),
            agent_type,
            status,
            conversation_id: row
                .get::<Option<String>>("conversation_id")
                .map(|s| Uuid::parse_str(&s).expect("invalid UUID in database")),
            created_at: parse_naive_datetime(&row.get::<String>("created_at")),
            completed_at: row
                .get::<Option<String>>("completed_at")
                .map(|s| parse_naive_datetime(&s)),
        }
    }
}

impl From<&mut Row<'_>> for ActiveAgent {
    fn from(row: &mut Row<'_>) -> Self {
        let agent_type_str: String = row.get("agent_type");
        let agent_type = agent_type_str.parse().unwrap_or(AgentType::Implementation);
        Self {
            agent_type,
            project_id: row.get::<i64>("project_id"),
            project_title: row.get("project_title"),
            task_id: row.get::<i64>("task_id"),
            task_title: row.get("task_title"),
            conversation_id: row
                .get::<String>("conversation_id")
                .parse()
                .expect("invalid UUID in database"),
            conversation_name: row.get("conversation_name"),
        }
    }
}

impl From<&mut Row<'_>> for TaskStatusSummary {
    fn from(row: &mut Row<'_>) -> Self {
        Self {
            project_id: row.get::<i64>("project_id"),
            project_title: row.get("project_title"),
            task_id: row.get::<i64>("task_id"),
            task_title: row.get("task_title"),
        }
    }
}

impl From<&mut Row<'_>> for UserModelConfig {
    fn from(row: &mut Row<'_>) -> Self {
        Self {
            id: row
                .get::<String>("id")
                .parse()
                .expect("invalid UUID in database"),
            user_id: row
                .get::<String>("user_id")
                .parse()
                .expect("invalid UUID in database"),
            name: row.get("name"),
            config_body: row.get("config_body"),
            harness: row.get("harness"),
            created_at: parse_naive_datetime(&row.get::<String>("created_at")),
            updated_at: parse_naive_datetime(&row.get::<String>("updated_at")),
        }
    }
}

impl From<&mut Row<'_>> for Prompt {
    fn from(row: &mut Row<'_>) -> Self {
        let kind_str: String = row.get("kind");
        let kind = kind_str.parse().unwrap_or(PromptKind::Snippet);
        Self {
            id: row
                .get::<String>("id")
                .parse()
                .expect("invalid UUID in database"),
            kind,
            title: row.get("title"),
            content: row.get("content"),
            tags: serde_json::from_str(&row.get::<String>("tags")).unwrap_or_default(),
            owner_user_id: row
                .get::<Option<String>>("owner_user_id")
                .map(|s| Uuid::parse_str(&s).expect("invalid UUID in database")),
            is_static: row.get::<i64>("is_static") != 0,
            is_shared: row.get::<i64>("is_shared") != 0,
            static_key: row.get("static_key"),
            created_at: parse_naive_datetime(&row.get::<String>("created_at")),
            updated_at: parse_naive_datetime(&row.get::<String>("updated_at")),
        }
    }
}

impl From<&mut Row<'_>> for PromptChild {
    fn from(row: &mut Row<'_>) -> Self {
        Self {
            parent_id: row
                .get::<String>("parent_id")
                .parse()
                .expect("invalid UUID in database"),
            child_id: row
                .get::<String>("child_id")
                .parse()
                .expect("invalid UUID in database"),
            position: row.get::<i64>("position"),
        }
    }
}

impl From<&mut Row<'_>> for PromptAssignment {
    fn from(row: &mut Row<'_>) -> Self {
        let agent_type_str: String = row.get("agent_type");
        let agent_type = agent_type_str.parse().unwrap_or(AgentType::Implementation);
        let scope_type_str: String = row.get("scope_type");
        let scope_type = scope_type_str.parse().unwrap_or(ScopeType::Global);
        Self {
            id: row
                .get::<String>("id")
                .parse()
                .expect("invalid UUID in database"),
            prompt_id: row
                .get::<String>("prompt_id")
                .parse()
                .expect("invalid UUID in database"),
            agent_type,
            scope_type,
            project_id: row.get::<Option<i64>>("project_id"),
            created_at: parse_naive_datetime(&row.get::<String>("created_at")),
        }
    }
}

impl From<&mut Row<'_>> for User {
    fn from(row: &mut Row<'_>) -> Self {
        Self {
            id: row
                .get::<String>("id")
                .parse()
                .expect("invalid UUID in database"),
            username: row.get("username"),
            oidc_subject: row.get("oidc_subject"),
            is_admin: row.get::<i64>("is_admin") != 0,
            is_technical: row.get::<i64>("is_technical") != 0,
            has_completed_onboarding: row.get::<i64>("has_completed_onboarding") != 0,
            git_name: row.get("git_name"),
            git_email: row.get("git_email"),
            api_key_hash: row.get("api_key_hash"),
            api_key_last_used_at: row.get("api_key_last_used_at"),
            is_active: row.get::<i64>("is_active") != 0,
            token_version: row.get::<i64>("token_version") as i32,
            created_at: row.get("created_at"),
            last_login: row.get("last_login"),
            scopes: row.get("scopes"),
        }
    }
}

impl From<&mut Row<'_>> for Group {
    fn from(row: &mut Row<'_>) -> Self {
        Self {
            id: row
                .get::<String>("id")
                .parse()
                .expect("invalid UUID in database"),
            name: row.get("name"),
            is_org: row.get::<i64>("is_org") != 0,
            is_oauth_scope: row.get::<i64>("is_oauth_scope") != 0,
            title: row.get("title"),
            description: row.get("description"),
            owner_id: row
                .get::<String>("owner_id")
                .parse()
                .expect("invalid UUID in database"),
            created_by: row.get("created_by"),
            created_at: parse_naive_datetime(&row.get::<String>("created_at")),
        }
    }
}

impl From<&mut Row<'_>> for GroupMember {
    fn from(row: &mut Row<'_>) -> Self {
        Self {
            id: row
                .get::<String>("id")
                .parse()
                .expect("invalid UUID in database"),
            group_id: row
                .get::<String>("group_id")
                .parse()
                .expect("invalid UUID in database"),
            user_id: row
                .get::<Option<String>>("user_id")
                .map(|s| Uuid::parse_str(&s).expect("invalid UUID in database")),
            member_group_id: row
                .get::<Option<String>>("member_group_id")
                .map(|s| Uuid::parse_str(&s).expect("invalid UUID in database")),
            level: row.get("level"),
            created_at: parse_naive_datetime(&row.get::<String>("created_at")),
        }
    }
}

impl From<&mut Row<'_>> for SessionDb {
    fn from(row: &mut Row<'_>) -> Self {
        Self {
            id: row
                .get::<String>("id")
                .parse()
                .expect("invalid UUID in database"),
            user_id: row
                .get::<String>("user_id")
                .parse()
                .expect("invalid UUID in database"),
            token_version: row.get::<i64>("token_version") as i32,
            refresh_token: row.get("refresh_token"),
            id_token: row.get("id_token"),
            access_token: row.get("access_token"),
            expires_at: row.get("expires_at"),
            created_at: row.get("created_at"),
        }
    }
}

impl From<&mut Row<'_>> for SystemHealthEntryDb {
    fn from(row: &mut Row<'_>) -> Self {
        Self {
            id: row.get::<i64>("id"),
            category: row.get("category"),
            resource: row.get("resource"),
            status: row.get("status"),
            detail: row.get("detail"),
            metadata: row.get("metadata"),
            created_at: row.get("created_at"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_status_display() {
        assert_eq!(TaskStatus::Pending.to_string(), "pending");
        assert_eq!(TaskStatus::InProgress.to_string(), "in_progress");
        assert_eq!(TaskStatus::InReview.to_string(), "in_review");
        assert_eq!(TaskStatus::Completed.to_string(), "completed");
    }

    #[test]
    fn test_agent_type_display() {
        assert_eq!(AgentType::Planification.to_string(), "planification");
        assert_eq!(AgentType::Implementation.to_string(), "implementation");
        assert_eq!(AgentType::Refinement.to_string(), "refinement");
        assert_eq!(AgentType::Review.to_string(), "review");
        assert_eq!(AgentType::Pr.to_string(), "pr");
        assert_eq!(AgentType::Yolo.to_string(), "yolo");
        assert_eq!(
            AgentType::ConversationTitle.to_string(),
            "conversation_title"
        );
    }

    #[test]
    fn test_agent_type_icon_mapping() {
        assert_eq!(AgentType::Planification.icon(), "file-document-outline");
        assert_eq!(AgentType::Implementation.icon(), "code-tags");
        assert_eq!(AgentType::Refinement.icon(), "creation-outline");
        assert_eq!(AgentType::Review.icon(), "checkbox-marked-circle-outline");
        assert_eq!(AgentType::Pr.icon(), "source-branch-plus");
        assert_eq!(AgentType::Yolo.icon(), "rocket");
        assert_eq!(AgentType::ConversationTitle.icon(), "message-text-outline");
    }

    #[test]
    fn test_run_status_display() {
        assert_eq!(RunStatus::Pending.to_string(), "pending");
        assert_eq!(RunStatus::Running.to_string(), "running");
        assert_eq!(RunStatus::Completed.to_string(), "completed");
        assert_eq!(RunStatus::Failed.to_string(), "failed");
        assert_eq!(RunStatus::Blocked.to_string(), "blocked");
    }

    #[test]
    fn test_project_serde_roundtrip() {
        let project = Project {
            id: 42,
            user_id: Uuid::new_v4(),
            name: "test-project".into(),
            repo_folder_path: "/tmp/repo".into(),
            subproject_path: None,
            tags: vec!["desktop-3d".into()],
            created_at: chrono::Utc::now().naive_utc(),
        };
        let json = serde_json::to_string(&project).unwrap();
        let deserialized: Project = serde_json::from_str(&json).unwrap();
        assert_eq!(project.id, deserialized.id);
        assert_eq!(project.name, deserialized.name);
        assert_eq!(project.tags, deserialized.tags);
    }

    #[test]
    fn test_task_agent_run_serde_roundtrip() {
        let run = TaskAgentRun {
            id: Uuid::new_v4(),
            task_id: 1,
            agent_type: AgentType::Implementation,
            status: RunStatus::Running,
            conversation_id: None,
            created_at: chrono::Utc::now().naive_utc(),
            completed_at: None,
        };
        let json = serde_json::to_string(&run).unwrap();
        let deserialized: TaskAgentRun = serde_json::from_str(&json).unwrap();
        assert_eq!(run.id, deserialized.id);
        assert_eq!(run.agent_type, deserialized.agent_type);
        assert_eq!(run.status, deserialized.status);
    }
}
