use crate::db::schema::{AgentType, Task};
use crate::orchestration::{NextAction, MAX_WORKFLOW_RUNS};

pub fn next_agent(task: &Task, current_agent: &AgentType) -> NextAction {
    match current_agent {
        AgentType::Planification => NextAction::Stop,
        _ if task.workflow_complete => NextAction::StartAgent(AgentType::Pr),
        _ if task.workflow_blocked => NextAction::Stop,
        _ if task.workflow_run_count >= MAX_WORKFLOW_RUNS => NextAction::Stop,
        _ if task.pr_agent_complete => NextAction::Terminal,
        AgentType::Implementation => NextAction::StartAgent(AgentType::Review),
        AgentType::Review => NextAction::StartAgent(AgentType::Implementation),
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

    #[test]
    fn test_planning_stops() {
        let task = make_task();
        let action = next_agent(&task, &AgentType::Planification);
        assert!(matches!(action, NextAction::Stop));
    }

    #[test]
    fn test_workflow_complete_triggers_pr() {
        let mut task = make_task();
        task.workflow_complete = true;
        let action = next_agent(&task, &AgentType::Review);
        assert!(matches!(action, NextAction::StartAgent(AgentType::Pr)));
    }

    #[test]
    fn test_workflow_blocked_stops() {
        let mut task = make_task();
        task.workflow_blocked = true;
        let action = next_agent(&task, &AgentType::Review);
        assert!(matches!(action, NextAction::Stop));
    }

    #[test]
    fn test_implementation_toggles_to_review() {
        let task = make_task();
        let action = next_agent(&task, &AgentType::Implementation);
        assert!(matches!(action, NextAction::StartAgent(AgentType::Review)));
    }

    #[test]
    fn test_review_toggles_to_implementation() {
        let task = make_task();
        let action = next_agent(&task, &AgentType::Review);
        assert!(matches!(
            action,
            NextAction::StartAgent(AgentType::Implementation)
        ));
    }

    #[test]
    fn test_pr_is_terminal() {
        let mut task = make_task();
        task.pr_agent_complete = true;
        let action = next_agent(&task, &AgentType::Pr);
        assert!(matches!(action, NextAction::Terminal));
    }

    #[test]
    fn test_iteration_cap_stops() {
        let mut task = make_task();
        task.workflow_run_count = 25;
        let action = next_agent(&task, &AgentType::Implementation);
        assert!(matches!(action, NextAction::Stop));
    }
}
