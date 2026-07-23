use crate::db::schema::Task;
use leptos::prelude::*;

fn status_badge_class(status: &str) -> &'static str {
    match status {
        "pending" => "is-light",
        "in_progress" => "is-info is-light",
        "in_review" => "is-warning is-light",
        "completed" => "is-success is-light",
        _ => "is-light",
    }
}

fn status_label(status: &str) -> &'static str {
    match status {
        "pending" => "Pending",
        "in_progress" => "In Progress",
        "in_review" => "In Review",
        "completed" => "Completed",
        _ => "Unknown",
    }
}

#[component]
pub fn TaskCard(task: Task) -> impl IntoView {
    let badge_class = status_badge_class(&task.status);
    let label = status_label(&task.status);
    let created = task.created_at.format("%Y-%m-%d").to_string();

    view! {
        <a href={format!("/webapp/projects/{}/tasks/{}", task.project_id, task.id)} class="box" style="display:block">
            <p class="title is-6">{task.title.clone()}</p>
            <div class="level">
                <div class="level-left">
                    <span class={format!("tag {}", badge_class)}>{label}</span>
                    <small class="has-text-grey">{created}</small>
                </div>
                <div class="level-right">
                    <button
                        class="button is-small is-danger is-outlined"
                        data-task-delete=""
                        data-task-id={task.id.to_string()}
                        data-project-id={task.project_id.to_string()}
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

    #[test]
    fn test_task_card_renders_title_and_status() {
        let task = make_task("pending");
        let html = leptos::view! { <TaskCard task /> }.to_html();
        assert!(html.contains("Test Task"));
        assert!(html.contains("Pending"));
        assert!(html.contains("2024-06-01"));
        assert!(html.contains("is-light"));
    }

    #[test]
    fn test_task_card_status_badges() {
        let statuses = [
            ("pending", "is-light", "Pending"),
            ("in_progress", "is-info is-light", "In Progress"),
            ("in_review", "is-warning is-light", "In Review"),
            ("completed", "is-success is-light", "Completed"),
        ];
        for (status, expected_class, expected_label) in &statuses {
            let task = make_task(status);
            let html = leptos::view! { <TaskCard task /> }.to_html();
            assert!(html.contains(expected_label), "label mismatch for {status}");
            assert!(html.contains(expected_class), "class mismatch for {status}");
        }
    }

    #[test]
    fn test_task_card_has_delete_button() {
        let task = make_task("pending");
        let html = leptos::view! { <TaskCard task /> }.to_html();
        assert!(html.contains("data-task-delete"));
        assert!(html.contains("mdi-trash-can"));
        assert!(html.contains("data-task-id=\"1\""));
        assert!(html.contains("data-project-id=\"1\""));
    }
}
