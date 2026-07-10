use crate::db::schema::{AgentType, Task};
use crate::orchestration::{NextAction, MAX_WORKFLOW_RUNS};
use crate::providers::registry::AgentConfigStatus;

pub fn next_agent(
    task: &Task,
    current_agent: &AgentType,
    config_statuses: &[AgentConfigStatus],
) -> NextAction {
    let is_configured = |agent_type: &AgentType| -> bool {
        let name = agent_type.to_string();
        config_statuses
            .iter()
            .any(|s| s.agent_type == name && s.configured)
    };

    match *current_agent {
        AgentType::Planification => NextAction::Stop,
        _ if task.pr_agent_complete => NextAction::Terminal,
        _ if task.workflow_complete => {
            if is_configured(&AgentType::Pr) {
                NextAction::StartAgent(AgentType::Pr)
            } else {
                NextAction::Terminal
            }
        }
        _ if task.workflow_blocked => NextAction::Stop,
        _ if task.workflow_run_count >= MAX_WORKFLOW_RUNS => NextAction::Stop,
        AgentType::Implementation => {
            if is_configured(&AgentType::Review) {
                NextAction::StartAgent(AgentType::Review)
            } else {
                NextAction::Stop
            }
        }
        AgentType::Review => {
            if is_configured(&AgentType::Implementation) {
                NextAction::StartAgent(AgentType::Implementation)
            } else {
                NextAction::Stop
            }
        }
        _ => NextAction::Stop,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::schema::Task;

    fn make_task() -> Task {
        Task {
            id: 1,
            project_id: 1,
            user_id: uuid::Uuid::new_v4(),
            title: "test".into(),
            status: "pending".into(),
            workflow_complete: false,
            workflow_blocked: false,
            workflow_run_count: 0,
            planification_complete: false,
            pr_agent_complete: false,
            refinement_complete: false,
            yolo_mode: false,
            created_at: chrono::Utc::now().naive_utc(),
        }
    }

    fn all_configured() -> Vec<AgentConfigStatus> {
        vec![
            AgentConfigStatus {
                agent_type: "planification".into(),
                configured: true,
                scope: None,
                label: None,
            },
            AgentConfigStatus {
                agent_type: "implementation".into(),
                configured: true,
                scope: None,
                label: None,
            },
            AgentConfigStatus {
                agent_type: "refinement".into(),
                configured: true,
                scope: None,
                label: None,
            },
            AgentConfigStatus {
                agent_type: "review".into(),
                configured: true,
                scope: None,
                label: None,
            },
            AgentConfigStatus {
                agent_type: "pr".into(),
                configured: true,
                scope: None,
                label: None,
            },
        ]
    }

    fn empty_configured() -> Vec<AgentConfigStatus> {
        vec![
            AgentConfigStatus {
                agent_type: "planification".into(),
                configured: false,
                scope: None,
                label: None,
            },
            AgentConfigStatus {
                agent_type: "implementation".into(),
                configured: false,
                scope: None,
                label: None,
            },
            AgentConfigStatus {
                agent_type: "refinement".into(),
                configured: false,
                scope: None,
                label: None,
            },
            AgentConfigStatus {
                agent_type: "review".into(),
                configured: false,
                scope: None,
                label: None,
            },
            AgentConfigStatus {
                agent_type: "pr".into(),
                configured: false,
                scope: None,
                label: None,
            },
        ]
    }

    #[test]
    fn test_planning_stops() {
        let task = make_task();
        let action = next_agent(&task, &AgentType::Planification, &all_configured());
        assert!(matches!(action, NextAction::Stop));
    }

    #[test]
    fn test_workflow_complete_triggers_pr() {
        let mut task = make_task();
        task.workflow_complete = true;
        let action = next_agent(&task, &AgentType::Review, &all_configured());
        assert!(matches!(action, NextAction::StartAgent(AgentType::Pr)));
    }

    #[test]
    fn test_workflow_complete_unconfigured_pr_returns_terminal() {
        let mut task = make_task();
        task.workflow_complete = true;
        let action = next_agent(&task, &AgentType::Review, &empty_configured());
        assert!(matches!(action, NextAction::Terminal));
    }

    #[test]
    fn test_workflow_blocked_stops() {
        let mut task = make_task();
        task.workflow_blocked = true;
        let action = next_agent(&task, &AgentType::Review, &all_configured());
        assert!(matches!(action, NextAction::Stop));
    }

    #[test]
    fn test_implementation_toggles_to_review() {
        let task = make_task();
        let action = next_agent(&task, &AgentType::Implementation, &all_configured());
        assert!(matches!(action, NextAction::StartAgent(AgentType::Review)));
    }

    #[test]
    fn test_implementation_skips_when_review_unconfigured() {
        let task = make_task();
        let configs = vec![
            AgentConfigStatus {
                agent_type: "review".into(),
                configured: false,
                scope: None,
                label: None,
            },
        ];
        let action = next_agent(&task, &AgentType::Implementation, &configs);
        assert!(matches!(action, NextAction::Stop));
    }

    #[test]
    fn test_review_toggles_to_implementation() {
        let task = make_task();
        let action = next_agent(&task, &AgentType::Review, &all_configured());
        assert!(matches!(
            action,
            NextAction::StartAgent(AgentType::Implementation)
        ));
    }

    #[test]
    fn test_review_skips_when_implementation_unconfigured() {
        let task = make_task();
        let configs = vec![
            AgentConfigStatus {
                agent_type: "implementation".into(),
                configured: false,
                scope: None,
                label: None,
            },
        ];
        let action = next_agent(&task, &AgentType::Review, &configs);
        assert!(matches!(action, NextAction::Stop));
    }

    #[test]
    fn test_pr_is_terminal() {
        let mut task = make_task();
        task.pr_agent_complete = true;
        let action = next_agent(&task, &AgentType::Pr, &all_configured());
        assert!(matches!(action, NextAction::Terminal));
    }

    #[test]
    fn test_iteration_cap_stops() {
        let mut task = make_task();
        task.workflow_run_count = 25;
        let action = next_agent(&task, &AgentType::Implementation, &all_configured());
        assert!(matches!(action, NextAction::Stop));
    }
}
