use crate::db::schema::{AgentType, Task};
use leptos::prelude::*;

#[derive(Clone)]
pub struct TaskCardData {
    pub task: Task,
    pub agent_types_run: Vec<AgentType>,
    pub running_agent: Option<AgentType>,
}

const CANONICAL_PHASE_ORDER: &[AgentType] = &[
    AgentType::Planification,
    AgentType::Implementation,
    AgentType::Review,
    AgentType::Refinement,
    AgentType::Pr,
];

fn phase_color(at: &AgentType) -> &'static str {
    match at {
        AgentType::Planification => "var(--bulma-info)",
        AgentType::Implementation => "var(--bulma-purple)",
        AgentType::Review => "var(--bulma-primary)",
        AgentType::Refinement => "var(--bulma-danger)",
        AgentType::Pr => "var(--bulma-success)",
        _ => "var(--bulma-grey-dark)",
    }
}

#[component]
pub fn TaskCard(data: TaskCardData) -> impl IntoView {
    let created = data.task.created_at.format("%Y-%m-%d").to_string();
    view! {
        <a href={format!("/webapp/projects/{}/tasks/{}", data.task.project_id, data.task.id)} class="card" style="display:block" data-task-id={data.task.id.to_string()}>
            <div class="card-header">
                <p class="card-header-title">{data.task.title.clone()}</p>
                <span class="card-header-icon">
                    <span class="tag is-light card-number-pill">{format!("#{}", data.task.id)}</span>
                </span>
            </div>
            <div class="card-content" style="padding:0.5rem">
                <div class="level is-mobile" style="margin-bottom:0">
                    <div class="level-left">
                        {CANONICAL_PHASE_ORDER.iter().filter(|at| data.agent_types_run.contains(at)).map(|at| {
                            let icon = at.icon();
                            let color = phase_color(at);
                            let pulse = matches!(&data.running_agent, Some(r) if r == at);
                            let pulse_class = if pulse { "is-pulse" } else { "" };
                            view! {
                                <span class={format!("icon is-small task-card-icon {}", pulse_class)} style={format!("color:{};margin:0.15rem", color)}>
                                    <i class={format!("mdi mdi-{}", icon)}></i>
                                </span>
                            }
                        }).collect::<Vec<_>>()}
                    </div>
                </div>
            </div>
            <div class="card-footer">
                <div class="card-footer-item" style="justify-content:flex-start;border-right:none">
                    <small class="has-text-grey">{created}</small>
                </div>
                <div class="card-footer-item" style="justify-content:flex-end">
                    <button
                        class="button is-small is-danger is-outlined"
                        data-task-delete=""
                        data-task-id={data.task.id.to_string()}
                        data-project-id={data.task.project_id.to_string()}
                        title="Delete task"
                    >
                        <span class="icon is-small"><i class="mdi mdi-trash-can"></i></span>
                    </button>
                </div>
            </div>
        </a>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDateTime;
    use uuid::Uuid;

    fn make_task(status: &str) -> Task {
        Task {
            id: 1,
            project_id: 1,
            user_id: Uuid::new_v4(),
            title: "Test Task".into(),
            status: status.into(),
            workflow_blocked: false,
            workflow_run_count: 0,
            yolo_mode: false,
            created_at: NaiveDateTime::parse_from_str("2024-06-01 12:00:00", "%Y-%m-%d %H:%M:%S")
                .unwrap(),
        }
    }

    fn make_data(
        status: &str,
        agent_types: Vec<AgentType>,
        running: Option<AgentType>,
    ) -> TaskCardData {
        TaskCardData {
            task: make_task(status),
            agent_types_run: agent_types,
            running_agent: running,
        }
    }

    #[test]
    fn test_task_card_renders_title_and_date() {
        let data = make_data("pending", vec![], None);
        let html = leptos::view! { <TaskCard data /> }.to_html();
        assert!(html.contains("Test Task"));
        assert!(html.contains("2024-06-01"));
        assert!(html.contains(r#"class="card""#));
        assert!(html.contains("card-header-title"));
        assert!(html.contains("card-footer"));
    }

    #[test]
    fn test_task_card_has_delete_button() {
        let data = make_data("pending", vec![], None);
        let html = leptos::view! { <TaskCard data /> }.to_html();
        assert!(html.contains("data-task-delete"));
        assert!(html.contains("mdi-trash-can"));
        assert!(html.contains("data-task-id=\"1\""));
        assert!(html.contains("data-project-id=\"1\""));
    }

    #[test]
    fn test_task_card_root_has_data_task_id() {
        let data = make_data("pending", vec![], None);
        let html = leptos::view! { <TaskCard data /> }.to_html();
        assert!(html.contains(r#"data-task-id="1""#));
        assert!(html.contains(r#"class="card""#));
        assert!(!html.contains(r#"class="box""#));
    }

    #[test]
    fn test_task_card_phase_icons() {
        let data = make_data(
            "pending",
            vec![AgentType::Planification, AgentType::Implementation],
            None,
        );
        let html = leptos::view! { <TaskCard data /> }.to_html();
        assert!(html.contains("mdi-file-document-outline"));
        assert!(html.contains("mdi-code-tags"));
        assert!(!html.contains("mdi-checkbox-marked-circle-outline"));
        assert!(!html.contains("mdi-creation-outline"));
        assert!(!html.contains("mdi-source-branch-plus"));
        assert!(html.contains("var(--bulma-info)"));
        assert!(html.contains("var(--bulma-purple)"));
    }

    #[test]
    fn test_task_card_pulse() {
        let data = make_data(
            "pending",
            vec![AgentType::Planification, AgentType::Implementation],
            Some(AgentType::Implementation),
        );
        let html = leptos::view! { <TaskCard data /> }.to_html();
        assert!(html.contains("is-pulse"));
        assert!(html.contains("mdi-code-tags"));
        assert!(html.contains("mdi-file-document-outline"));
    }

    #[test]
    fn test_task_card_no_pulse_when_not_running() {
        let data = make_data("pending", vec![AgentType::Planification], None);
        let html = leptos::view! { <TaskCard data /> }.to_html();
        assert!(!html.contains("is-pulse"));
    }

    #[test]
    fn test_task_card_header_has_number_pill() {
        let data = make_data("pending", vec![], None);
        let html = leptos::view! { <TaskCard data /> }.to_html();
        assert!(html.contains("card-header-icon"));
        assert!(html.contains("card-number-pill"));
        assert!(html.contains("#1"));
        assert!(html.contains("card-header-title"));
    }
}
