use crate::db::schema::{AgentType, Task};
use crate::orchestration::{NextAction, MAX_WORKFLOW_RUNS};
use crate::providers::registry::AgentConfigStatus;

/// Get the label for an agent type for display in the UI.
pub fn agent_type_label(agent_type: &AgentType) -> &'static str {
    match agent_type {
        AgentType::Planification => "Planification",
        AgentType::Implementation => "Implementation",
        AgentType::Refinement => "Refinement",
        AgentType::Review => "Review",
        AgentType::Pr => "PR",
        AgentType::Yolo => "Yolo",
        AgentType::ConversationTitle => "Conversation Title",
    }
}

pub fn next_agent(
    task: &Task,
    current_agent: &AgentType,
    config_statuses: &[AgentConfigStatus],
    review_ready: bool,
) -> NextAction {
    let is_configured = |agent_type: &AgentType| -> bool {
        let name = agent_type.to_string();
        config_statuses
            .iter()
            .any(|s| s.agent_type == name && s.configured)
    };

    match *current_agent {
        AgentType::Planification => NextAction::Stop, // human gate
        _ if task.workflow_blocked => NextAction::Stop, // server-only cap marker
        _ if task.workflow_run_count >= MAX_WORKFLOW_RUNS => NextAction::Stop,
        AgentType::Implementation => {
            if is_configured(&AgentType::Review) {
                NextAction::StartAgent(AgentType::Review)
            } else {
                NextAction::Stop
            }
        }
        AgentType::Review => {
            if review_ready {
                if is_configured(&AgentType::Refinement) {
                    NextAction::StartAgent(AgentType::Refinement)
                } else if is_configured(&AgentType::Pr) {
                    NextAction::StartAgent(AgentType::Pr)
                } else {
                    NextAction::Terminal
                }
            } else if is_configured(&AgentType::Implementation) {
                NextAction::StartAgent(AgentType::Implementation)
            } else {
                NextAction::Stop
            }
        }
        AgentType::Refinement => {
            if is_configured(&AgentType::Pr) {
                NextAction::StartAgent(AgentType::Pr)
            } else {
                NextAction::Terminal
            }
        }
        AgentType::Pr => NextAction::Terminal,
        _ => NextAction::Stop,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::schema::Task;
    use crate::providers::registry::AgentConfigStatus;

    fn make_task() -> Task {
        Task {
            id: 1,
            project_id: 1,
            user_id: uuid::Uuid::new_v4(),
            title: "test".into(),
            status: "pending".into(),
            workflow_blocked: false,
            workflow_run_count: 0,
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
        let action = next_agent(&task, &AgentType::Planification, &all_configured(), false);
        assert!(matches!(action, NextAction::Stop));
    }

    #[test]
    fn test_workflow_blocked_stops() {
        let mut task = make_task();
        task.workflow_blocked = true;
        let action = next_agent(&task, &AgentType::Review, &all_configured(), false);
        assert!(matches!(action, NextAction::Stop));
    }

    #[test]
    fn test_implementation_toggles_to_review() {
        let task = make_task();
        let action = next_agent(&task, &AgentType::Implementation, &all_configured(), false);
        assert!(matches!(action, NextAction::StartAgent(AgentType::Review)));
    }

    #[test]
    fn test_implementation_skips_when_review_unconfigured() {
        let task = make_task();
        let action = next_agent(
            &task,
            &AgentType::Implementation,
            &empty_configured(),
            false,
        );
        assert!(matches!(action, NextAction::Stop));
    }

    #[test]
    fn test_review_ready_toggles_to_refinement() {
        let task = make_task();
        let action = next_agent(&task, &AgentType::Review, &all_configured(), true);
        assert!(matches!(
            action,
            NextAction::StartAgent(AgentType::Refinement)
        ));
    }

    #[test]
    fn test_review_ready_skips_to_pr_when_refinement_unconfigured() {
        let task = make_task();
        let mut configs = all_configured();
        for c in configs.iter_mut() {
            if c.agent_type == "refinement" {
                c.configured = false;
            }
        }
        let action = next_agent(&task, &AgentType::Review, &configs, true);
        assert!(matches!(action, NextAction::StartAgent(AgentType::Pr)));
    }

    #[test]
    fn test_review_ready_terminal_when_nothing_configured() {
        let task = make_task();
        let action = next_agent(&task, &AgentType::Review, &empty_configured(), true);
        assert!(matches!(action, NextAction::Terminal));
    }

    #[test]
    fn test_review_not_ready_toggles_to_implementation() {
        let task = make_task();
        let action = next_agent(&task, &AgentType::Review, &all_configured(), false);
        assert!(matches!(
            action,
            NextAction::StartAgent(AgentType::Implementation)
        ));
    }

    #[test]
    fn test_review_not_ready_skips_when_implementation_unconfigured() {
        let task = make_task();
        let action = next_agent(&task, &AgentType::Review, &empty_configured(), false);
        assert!(matches!(action, NextAction::Stop));
    }

    #[test]
    fn test_refinement_toggles_to_pr() {
        let task = make_task();
        let action = next_agent(&task, &AgentType::Refinement, &all_configured(), false);
        assert!(matches!(action, NextAction::StartAgent(AgentType::Pr)));
    }

    #[test]
    fn test_refinement_terminal_when_pr_unconfigured() {
        let task = make_task();
        let action = next_agent(&task, &AgentType::Refinement, &empty_configured(), false);
        assert!(matches!(action, NextAction::Terminal));
    }

    #[test]
    fn test_pr_is_terminal() {
        let task = make_task();
        let action = next_agent(&task, &AgentType::Pr, &all_configured(), false);
        assert!(matches!(action, NextAction::Terminal));
    }

    #[test]
    fn test_iteration_cap_stops() {
        let mut task = make_task();
        task.workflow_run_count = 25;
        let action = next_agent(&task, &AgentType::Implementation, &all_configured(), false);
        assert!(matches!(action, NextAction::Stop));
    }

    #[test]
    fn test_agent_type_label() {
        assert_eq!(agent_type_label(&AgentType::Planification), "Planification");
        assert_eq!(
            agent_type_label(&AgentType::Implementation),
            "Implementation"
        );
        assert_eq!(agent_type_label(&AgentType::Review), "Review");
        assert_eq!(agent_type_label(&AgentType::Pr), "PR");
    }
}
