use crate::db::schema::{AgentType, RunStatus, Task, TaskAgentRun};
use crate::orchestration::{NextAction, MAX_WORKFLOW_RUNS};
use crate::providers::registry::AgentConfigStatus;

/// Determine which phase should run next based on task state and existing runs.
/// Returns the AgentType that should be started next, or None if the workflow is complete.
pub fn determine_next_phase(task: &Task, runs: &[TaskAgentRun]) -> Option<AgentType> {
    // If workflow is complete and PR not done, start PR
    if task.workflow_complete && !task.pr_agent_complete {
        return Some(AgentType::Pr);
    }

    // If PR is complete, we're done
    if task.pr_agent_complete {
        return None;
    }

    // If blocked or at iteration cap, stop
    if task.workflow_blocked || task.workflow_run_count >= MAX_WORKFLOW_RUNS {
        return None;
    }

    // Check if planification has been completed
    let has_successful_planification = runs.iter().any(|r| {
        r.agent_type == AgentType::Planification && r.status == RunStatus::Completed
    });

    if !has_successful_planification {
        // Check if planification is currently running
        let planification_running = runs.iter().any(|r| {
            r.agent_type == AgentType::Planification && r.status == RunStatus::Running
        });
        if planification_running {
            return None; // Wait for it to finish
        }
        return Some(AgentType::Planification);
    }

    // Planification is done, now we need implementation/review cycle
    // Find the most recent run
    let most_recent = runs.iter().max_by_key(|r| r.created_at);

    match most_recent {
        Some(run) if run.status == RunStatus::Running => {
            // Something is running, wait
            None
        }
        Some(run) => {
            // Check what ran last and determine next
            match run.agent_type {
                AgentType::Planification => Some(AgentType::Implementation),
                AgentType::Implementation => Some(AgentType::Review),
                AgentType::Review => Some(AgentType::Implementation),
                AgentType::Pr => {
                    if run.status == RunStatus::Completed {
                        None // Done
                    } else {
                        Some(AgentType::Pr) // Retry PR
                    }
                }
                _ => Some(AgentType::Implementation),
            }
        }
        None => {
            // No runs yet but planification is complete (shouldn't happen, but be safe)
            Some(AgentType::Implementation)
        }
    }
}

/// Get the label for an agent type for display in the UI.
pub fn agent_type_label(agent_type: &AgentType) -> &'static str {
    match agent_type {
        AgentType::Planification => "Planification",
        AgentType::Implementation => "Implementation",
        AgentType::Refinement => "Refinement",
        AgentType::Review => "Review",
        AgentType::Pr => "PR",
        AgentType::Yolo => "Yolo",
    }
}

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
    use chrono::NaiveDateTime;
    use uuid::Uuid;

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
        let configs = vec![AgentConfigStatus {
            agent_type: "review".into(),
            configured: false,
            scope: None,
            label: None,
        }];
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
        let configs = vec![AgentConfigStatus {
            agent_type: "implementation".into(),
            configured: false,
            scope: None,
            label: None,
        }];
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

    // Tests for determine_next_phase

    fn make_run(agent_type: AgentType, status: RunStatus) -> TaskAgentRun {
        TaskAgentRun {
            id: Uuid::new_v4(),
            task_id: 1,
            agent_type,
            status,
            conversation_id: Some(Uuid::new_v4()),
            created_at: NaiveDateTime::parse_from_str("2024-06-01 12:00:00", "%Y-%m-%d %H:%M:%S")
                .unwrap(),
            completed_at: None,
        }
    }

    #[test]
    fn test_determine_next_phase_no_runs_starts_planification() {
        let task = make_task();
        let runs = vec![];
        let next = determine_next_phase(&task, &runs);
        assert_eq!(next, Some(AgentType::Planification));
    }

    #[test]
    fn test_determine_next_phase_planification_running_waits() {
        let task = make_task();
        let runs = vec![make_run(AgentType::Planification, RunStatus::Running)];
        let next = determine_next_phase(&task, &runs);
        assert_eq!(next, None);
    }

    #[test]
    fn test_determine_next_phase_planification_done_starts_implementation() {
        let task = make_task();
        let runs = vec![make_run(AgentType::Planification, RunStatus::Completed)];
        let next = determine_next_phase(&task, &runs);
        assert_eq!(next, Some(AgentType::Implementation));
    }

    #[test]
    fn test_determine_next_phase_implementation_done_starts_review() {
        let task = make_task();
        let runs = vec![
            make_run(AgentType::Planification, RunStatus::Completed),
            make_run(AgentType::Implementation, RunStatus::Completed),
        ];
        let next = determine_next_phase(&task, &runs);
        assert_eq!(next, Some(AgentType::Review));
    }

    #[test]
    fn test_determine_next_phase_review_done_starts_implementation() {
        let task = make_task();
        let runs = vec![
            make_run(AgentType::Planification, RunStatus::Completed),
            make_run(AgentType::Implementation, RunStatus::Completed),
            make_run(AgentType::Review, RunStatus::Completed),
        ];
        let next = determine_next_phase(&task, &runs);
        assert_eq!(next, Some(AgentType::Implementation));
    }

    #[test]
    fn test_determine_next_phase_workflow_complete_starts_pr() {
        let mut task = make_task();
        task.workflow_complete = true;
        let runs = vec![
            make_run(AgentType::Planification, RunStatus::Completed),
            make_run(AgentType::Implementation, RunStatus::Completed),
        ];
        let next = determine_next_phase(&task, &runs);
        assert_eq!(next, Some(AgentType::Pr));
    }

    #[test]
    fn test_determine_next_phase_pr_complete_returns_none() {
        let mut task = make_task();
        task.workflow_complete = true;
        task.pr_agent_complete = true;
        let runs = vec![
            make_run(AgentType::Planification, RunStatus::Completed),
            make_run(AgentType::Pr, RunStatus::Completed),
        ];
        let next = determine_next_phase(&task, &runs);
        assert_eq!(next, None);
    }

    #[test]
    fn test_determine_next_phase_blocked_returns_none() {
        let mut task = make_task();
        task.workflow_blocked = true;
        let runs = vec![make_run(AgentType::Planification, RunStatus::Completed)];
        let next = determine_next_phase(&task, &runs);
        assert_eq!(next, None);
    }

    #[test]
    fn test_determine_next_phase_at_cap_returns_none() {
        let mut task = make_task();
        task.workflow_run_count = 25;
        let runs = vec![make_run(AgentType::Planification, RunStatus::Completed)];
        let next = determine_next_phase(&task, &runs);
        assert_eq!(next, None);
    }

    #[test]
    fn test_determine_next_phase_implementation_running_waits() {
        let task = make_task();
        let runs = vec![
            make_run(AgentType::Planification, RunStatus::Completed),
            make_run(AgentType::Implementation, RunStatus::Running),
        ];
        let next = determine_next_phase(&task, &runs);
        assert_eq!(next, None);
    }

    #[test]
    fn test_determine_next_phase_failed_planification_restarts() {
        let task = make_task();
        let runs = vec![make_run(AgentType::Planification, RunStatus::Failed)];
        let next = determine_next_phase(&task, &runs);
        assert_eq!(next, Some(AgentType::Planification));
    }

    #[test]
    fn test_agent_type_label() {
        assert_eq!(agent_type_label(&AgentType::Planification), "Planification");
        assert_eq!(agent_type_label(&AgentType::Implementation), "Implementation");
        assert_eq!(agent_type_label(&AgentType::Review), "Review");
        assert_eq!(agent_type_label(&AgentType::Pr), "PR");
    }
}
