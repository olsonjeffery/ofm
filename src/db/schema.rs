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
        }
    }
}

impl std::str::FromStr for AgentType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "planification" => Ok(Self::Planification),
            "implementation" => Ok(Self::Implementation),
            "refinement" => Ok(Self::Refinement),
            "review" => Ok(Self::Review),
            "pr" => Ok(Self::Pr),
            "yolo" => Ok(Self::Yolo),
            _ => Err(format!("invalid agent type: '{s}'")),
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
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub repo_folder_path: String,
    pub subproject_path: Option<String>,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: Uuid,
    pub project_id: Uuid,
    pub user_id: Uuid,
    pub title: String,
    pub status: String,
    pub workflow_complete: bool,
    pub workflow_blocked: bool,
    pub workflow_run_count: i32,
    pub planification_complete: bool,
    pub pr_agent_complete: bool,
    pub refinement_complete: bool,
    pub yolo_mode: bool,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub id: Uuid,
    pub task_id: Uuid,
    pub omp_session_id: Option<String>,
    pub model: String,
    pub effort: String,
    pub name: Option<String>,
    pub created_at: NaiveDateTime,
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
    pub project_id: Option<Uuid>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskAgentRun {
    pub id: Uuid,
    pub task_id: Uuid,
    pub agent_type: AgentType,
    pub status: RunStatus,
    pub conversation_id: Option<Uuid>,
    pub created_at: NaiveDateTime,
    pub completed_at: Option<NaiveDateTime>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub project_key: String,
    pub session_id: String,
    pub seq: i32,
    pub entry_json: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Worktree {
    pub id: Uuid,
    pub project_uuid: Uuid,
    pub task_uuid: Uuid,
    pub project_id: u32,
    pub task_id: u32,
    pub worktree_path: String,
    pub repo_path: String,
    pub branch: String,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub project_key: String,
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
    pub token_version: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMember {
    pub id: Uuid,
    pub project_id: Uuid,
    pub user_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSetting {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserAgentModelSetting {
    pub user_id: Uuid,
    pub settings_json: serde_json::Value,
}

// hiqlite Row conversions

fn uuid_from_row(row: &mut Row<'_>, col: &str) -> Uuid {
    let s: String = row.get(col);
    Uuid::parse_str(&s).expect("invalid UUID in database")
}

fn parse_naive_datetime(s: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").unwrap_or_default()
}

impl From<&mut Row<'_>> for Project {
    fn from(row: &mut Row<'_>) -> Self {
        Self {
            id: uuid_from_row(row, "id"),
            user_id: uuid_from_row(row, "user_id"),
            name: row.get("name"),
            repo_folder_path: row.get("repo_folder_path"),
            subproject_path: row.get("subproject_path"),
            created_at: parse_naive_datetime(&row.get::<String>("created_at")),
        }
    }
}

impl From<&mut Row<'_>> for Task {
    fn from(row: &mut Row<'_>) -> Self {
        Self {
            id: uuid_from_row(row, "id"),
            project_id: uuid_from_row(row, "project_id"),
            user_id: uuid_from_row(row, "user_id"),
            title: row.get("title"),
            status: row.get("status"),
            workflow_complete: row.get::<i64>("workflow_complete") != 0,
            workflow_blocked: row.get::<i64>("workflow_blocked") != 0,
            workflow_run_count: row.get::<i64>("workflow_run_count") as i32,
            planification_complete: row.get::<i64>("planification_complete") != 0,
            pr_agent_complete: row.get::<i64>("pr_agent_complete") != 0,
            refinement_complete: row.get::<i64>("refinement_complete") != 0,
            yolo_mode: row.get::<i64>("yolo_mode") != 0,
            created_at: parse_naive_datetime(&row.get::<String>("created_at")),
        }
    }
}

impl From<&mut Row<'_>> for Worktree {
    fn from(row: &mut Row<'_>) -> Self {
        Self {
            id: uuid_from_row(row, "id"),
            project_uuid: uuid_from_row(row, "project_uuid"),
            task_uuid: uuid_from_row(row, "task_uuid"),
            project_id: row.get::<i64>("project_id") as u32,
            task_id: row.get::<i64>("task_id") as u32,
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
            project_key: row.get("project_key"),
            session_id: row.get("session_id"),
            seq: row.get::<i64>("seq") as i32,
            entry_json: serde_json::from_str(&row.get::<String>("entry_json")).unwrap_or_default(),
        }
    }
}

impl From<&mut Row<'_>> for SessionSummary {
    fn from(row: &mut Row<'_>) -> Self {
        Self {
            project_key: row.get("project_key"),
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
            id: uuid_from_row(row, "id"),
            task_id: uuid_from_row(row, "task_id"),
            omp_session_id: row.get("omp_session_id"),
            model: row.get("model"),
            effort: row.get("effort"),
            name: row.get("name"),
            created_at: parse_naive_datetime(&row.get::<String>("created_at")),
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
            id: uuid_from_row(row, "id"),
            agent_type,
            harness: row.get("harness"),
            provider_config_ref: row.get("provider_config_ref"),
            scope_type,
            user_id: row
                .get::<Option<String>>("user_id")
                .map(|s| Uuid::parse_str(&s).expect("invalid UUID in database")),
            project_id: row
                .get::<Option<String>>("project_id")
                .map(|s| Uuid::parse_str(&s).expect("invalid UUID in database")),
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
            id: uuid_from_row(row, "id"),
            task_id: uuid_from_row(row, "task_id"),
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

impl From<&mut Row<'_>> for User {
    fn from(row: &mut Row<'_>) -> Self {
        Self {
            id: uuid_from_row(row, "id"),
            username: row.get("username"),
            oidc_subject: row.get("oidc_subject"),
            is_admin: row.get::<i64>("is_admin") != 0,
            is_technical: row.get::<i64>("is_technical") != 0,
            has_completed_onboarding: row.get::<i64>("has_completed_onboarding") != 0,
            git_name: row.get("git_name"),
            git_email: row.get("git_email"),
            api_key_hash: row.get("api_key_hash"),
            api_key_last_used_at: row.get("api_key_last_used_at"),
            token_version: row.get::<i64>("token_version") as i32,
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
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            name: "test-project".into(),
            repo_folder_path: "/tmp/repo".into(),
            subproject_path: None,
            created_at: chrono::Utc::now().naive_utc(),
        };
        let json = serde_json::to_string(&project).unwrap();
        let deserialized: Project = serde_json::from_str(&json).unwrap();
        assert_eq!(project.id, deserialized.id);
        assert_eq!(project.name, deserialized.name);
    }

    #[test]
    fn test_task_agent_run_serde_roundtrip() {
        let run = TaskAgentRun {
            id: Uuid::new_v4(),
            task_id: Uuid::new_v4(),
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
