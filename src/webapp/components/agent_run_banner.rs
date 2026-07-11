use crate::db::schema::{AgentType, RunStatus, Task, TaskAgentRun};
use crate::orchestration::state_machine::{agent_type_label, determine_next_phase};
use crate::providers::registry::AgentConfigStatus;
use leptos::prelude::*;

fn run_status_class(status: &RunStatus) -> &'static str {
    match status {
        RunStatus::Pending => "is-light",
        RunStatus::Running => "is-info",
        RunStatus::Completed => "is-success",
        RunStatus::Failed => "is-danger",
        RunStatus::Blocked => "is-warning",
    }
}

fn agent_type_value(agent_type: &AgentType) -> &'static str {
    match agent_type {
        AgentType::Planification => "planification",
        AgentType::Implementation => "implementation",
        AgentType::Refinement => "refinement",
        AgentType::Review => "review",
        AgentType::Pr => "pr",
        AgentType::Yolo => "yolo",
    }
}

fn agent_type_label_str(agent_type: &str) -> &'static str {
    match agent_type {
        "planification" => "Planification",
        "implementation" => "Implementation",
        "refinement" => "Refinement",
        "review" => "Review",
        "pr" => "PR",
        _ => "Unknown",
    }
}

#[component]
pub fn AgentRunBanner(
    task: Task,
    agent_config_statuses: Vec<AgentConfigStatus>,
    current_run: Option<TaskAgentRun>,
    agent_runs: Vec<TaskAgentRun>,
) -> impl IntoView {
    let task_id = task.id.to_string();

    let (banner_class, banner_text) = if let Some(ref run) = current_run {
        match run.status {
            RunStatus::Blocked => {
                let reason = format!(
                    "Skipped — no model configured for {}",
                    agent_type_label(&run.agent_type)
                );
                ("is-warning", reason)
            }
            RunStatus::Failed => ("is-danger", "Agent run failed".to_string()),
            RunStatus::Completed => ("is-success", "Agent run completed".to_string()),
            RunStatus::Running => ("is-info", "Agent run in progress...".to_string()),
            RunStatus::Pending => ("is-light", "Agent run pending".to_string()),
        }
    } else {
        ("is-light", "No active agent run".to_string())
    };

    let start_disabled = current_run
        .as_ref()
        .map(|r| r.status == RunStatus::Running)
        .unwrap_or(false);

    let next_phase = determine_next_phase(&task, &agent_runs);
    let (btn_label, next_phase_value) = match next_phase {
        Some(ref phase) => (
            format!("Start {}", agent_type_label(phase)),
            agent_type_value(phase).to_string(),
        ),
        None => ("Workflow Complete".to_string(), String::new()),
    };
    let btn_disabled = start_disabled || next_phase.is_none();

    let statuses = agent_config_statuses.clone();
    let task_id_clone = task_id.clone();

    view! {
        <div class={format!("notification {}", banner_class)} style="margin-bottom:0.5rem;padding:0.75rem">
            <div class="level is-mobile" style="margin-bottom:0">
                <div class="level-left">
                    <span>{banner_text}</span>
                </div>
                <div class="level-right">
                    <div class="tags">
                        {statuses.into_iter().map(|s| {
                            let configured = s.configured;
                            let tag_class = if configured { "is-success" } else { "is-light" };
                            let label = agent_type_label_str(&s.agent_type).to_string();
                            let title = if configured {
                                format!("configured: {}", s.label.clone().unwrap_or_default())
                            } else {
                                "not configured".to_string()
                            };
                            let agent_type = s.agent_type.clone();
                            let tid = task_id_clone.clone();
                            let label_for_title = label.clone();
                            if configured {
                                leptos::view! {
                                    <span
                                        class={format!("tag {} cursor-pointer", tag_class)}
                                        title={format!("Click to start {}", label_for_title)}
                                        data-agent-type={agent_type}
                                        data-task-id={tid}
                                        style="cursor:pointer"
                                    >
                                        {label}
                                    </span>
                                }.into_any()
                            } else {
                                leptos::view! {
                                    <span class={format!("tag {}", tag_class)} title={title}>
                                        {label}
                                    </span>
                                }.into_any()
                            }
                        }).collect::<Vec<_>>()}
                    </div>
                    <button
                        class="button is-small is-primary"
                        id="start-agent-run-btn"
                        data-task-id={task_id}
                        data-next-phase={next_phase_value}
                        disabled=btn_disabled
                    >
                        {btn_label}
                    </button>
                </div>
            </div>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDateTime;

    fn make_task() -> Task {
        Task {
            id: 1,
            project_id: 1,
            user_id: uuid::Uuid::new_v4(),
            title: "Test Task".into(),
            status: "pending".into(),
            workflow_complete: false,
            workflow_blocked: false,
            workflow_run_count: 0,
            planification_complete: false,
            pr_agent_complete: false,
            refinement_complete: false,
            yolo_mode: false,
            created_at: NaiveDateTime::parse_from_str("2024-06-01 12:00:00", "%Y-%m-%d %H:%M:%S")
                .unwrap(),
        }
    }

    fn make_run(status: RunStatus) -> TaskAgentRun {
        TaskAgentRun {
            id: uuid::Uuid::new_v4(),
            task_id: 1,
            agent_type: crate::db::schema::AgentType::Implementation,
            status,
            conversation_id: Some(uuid::Uuid::new_v4()),
            created_at: NaiveDateTime::parse_from_str("2024-06-01 12:00:00", "%Y-%m-%d %H:%M:%S")
                .unwrap(),
            completed_at: None,
        }
    }

    fn make_run_with_type(agent_type: AgentType, status: RunStatus) -> TaskAgentRun {
        TaskAgentRun {
            id: uuid::Uuid::new_v4(),
            task_id: 1,
            agent_type,
            status,
            conversation_id: Some(uuid::Uuid::new_v4()),
            created_at: NaiveDateTime::parse_from_str("2024-06-01 12:00:00", "%Y-%m-%d %H:%M:%S")
                .unwrap(),
            completed_at: None,
        }
    }

    #[test]
    fn test_banner_shows_no_active_run() {
        let task = make_task();
        let html =
            leptos::view! { <AgentRunBanner task agent_config_statuses=vec![] current_run=None agent_runs=vec![] /> }
                .to_html();
        assert!(html.contains("No active agent run"));
        assert!(html.contains("Start Planification"));
    }

    #[test]
    fn test_banner_shows_running() {
        let task = make_task();
        let run = Some(make_run(RunStatus::Running));
        let html =
            leptos::view! { <AgentRunBanner task agent_config_statuses=vec![] current_run=run agent_runs=vec![] /> }
                .to_html();
        assert!(html.contains("in progress"));
    }

    #[test]
    fn test_banner_shows_blocked_with_reason() {
        let task = make_task();
        let run = Some(make_run(RunStatus::Blocked));
        let html =
            leptos::view! { <AgentRunBanner task agent_config_statuses=vec![] current_run=run agent_runs=vec![] /> }
                .to_html();
        assert!(html.contains("Skipped"));
        assert!(html.contains("no model configured"));
    }

    #[test]
    fn test_banner_shows_configured_tags() {
        let task = make_task();
        let statuses = vec![
            AgentConfigStatus {
                agent_type: "implementation".into(),
                configured: true,
                scope: Some("user".into()),
                label: Some("gpt-4".into()),
            },
            AgentConfigStatus {
                agent_type: "review".into(),
                configured: false,
                scope: None,
                label: None,
            },
        ];
        let html = leptos::view! { <AgentRunBanner task agent_config_statuses=statuses current_run=None agent_runs=vec![] /> }.to_html();
        assert!(html.contains("Implementation"));
        assert!(html.contains("Review"));
        assert!(html.contains("is-success"));
        assert!(html.contains("is-light"));
    }

    #[test]
    fn test_banner_button_shows_next_phase() {
        let task = make_task();
        let html =
            leptos::view! { <AgentRunBanner task agent_config_statuses=vec![] current_run=None agent_runs=vec![] /> }
                .to_html();
        assert!(html.contains("Start Planification"));
        assert!(html.contains("data-next-phase=\"planification\""));
    }

    #[test]
    fn test_banner_button_shows_implementation_after_planification() {
        let task = make_task();
        let runs = vec![make_run_with_type(
            AgentType::Planification,
            RunStatus::Completed,
        )];
        let html =
            leptos::view! { <AgentRunBanner task agent_config_statuses=vec![] current_run=None agent_runs=runs /> }
                .to_html();
        assert!(html.contains("Start Implementation"));
        assert!(html.contains("data-next-phase=\"implementation\""));
    }

    #[test]
    fn test_banner_button_disabled_when_running() {
        let task = make_task();
        let run = Some(make_run(RunStatus::Running));
        let html =
            leptos::view! { <AgentRunBanner task agent_config_statuses=vec![] current_run=run agent_runs=vec![] /> }
                .to_html();
        assert!(html.contains("disabled"));
    }

    #[test]
    fn test_banner_clickable_tags_for_configured() {
        let task = make_task();
        let statuses = vec![AgentConfigStatus {
            agent_type: "implementation".into(),
            configured: true,
            scope: Some("user".into()),
            label: Some("gpt-4".into()),
        }];
        let html = leptos::view! { <AgentRunBanner task agent_config_statuses=statuses current_run=None agent_runs=vec![] /> }.to_html();
        assert!(html.contains("cursor:pointer"));
        assert!(html.contains("data-agent-type=\"implementation\""));
    }
}
