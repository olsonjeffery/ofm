#![allow(dead_code)]

use chrono::NaiveDateTime;
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
    pub created_at: NaiveDateTime,
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
