use leptos::prelude::*;
use crate::db::schema::Project;

#[derive(Debug, Clone, Default)]
pub struct TaskCounts {
    pub pending: i32,
    pub in_progress: i32,
    pub in_review: i32,
    pub completed: i32,
}

#[component]
pub fn ProjectCard(project: Project, task_counts: TaskCounts) -> impl IntoView {
    let total = task_counts.pending + task_counts.in_progress
        + task_counts.in_review + task_counts.completed;
    view! {
        <a href={format!("/webapp/projects/{}", project.id)} class="card">
            <div class="card-content">
                <p class="title is-4">{project.name.clone()}</p>
                <p class="subtitle is-6">{project.repo_folder_path.clone()}</p>
                <div class="tags">
                    <span class="tag">{format!("{} tasks", total)}</span>
                    {if task_counts.pending > 0 {
                        view! { <span class="tag is-light">{format!("{} pending", task_counts.pending)}</span> }.into_any()
                    } else { "".into_any() }}
                    {if task_counts.in_progress > 0 {
                        view! { <span class="tag is-info is-light">{format!("{} in progress", task_counts.in_progress)}</span> }.into_any()
                    } else { "".into_any() }}
                    {if task_counts.in_review > 0 {
                        view! { <span class="tag is-warning is-light">{format!("{} in review", task_counts.in_review)}</span> }.into_any()
                    } else { "".into_any() }}
                    {if task_counts.completed > 0 {
                        view! { <span class="tag is-success is-light">{format!("{} completed", task_counts.completed)}</span> }.into_any()
                    } else { "".into_any() }}
                </div>
            </div>
        </a>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDateTime;

    fn make_project() -> Project {
        Project {
            id: uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap(),
            user_id: uuid::Uuid::new_v4(),
            name: "Test Project".into(),
            repo_folder_path: "/tmp/test-repo".into(),
            subproject_path: None,
            created_at: NaiveDateTime::parse_from_str("2024-01-15 10:00:00", "%Y-%m-%d %H:%M:%S").unwrap(),
        }
    }

    #[test]
    fn test_project_card_renders_name() {
        let project = make_project();
        let counts = TaskCounts { pending: 3, in_progress: 1, in_review: 2, completed: 5 };
        let html = leptos::view! { <ProjectCard project task_counts=counts /> }.to_html();
        assert!(html.contains("Test Project"));
        assert!(html.contains("/tmp/test-repo"));
        assert!(html.contains("11 tasks"));
        assert!(html.contains("3 pending"));
        assert!(html.contains("1 in progress"));
        assert!(html.contains("2 in review"));
        assert!(html.contains("5 completed"));
        assert!(html.contains("/webapp/projects/00000000-0000-0000-0000-000000000001"));
    }

    #[test]
    fn test_project_card_hides_zero_counts() {
        let project = make_project();
        let counts = TaskCounts::default();
        let html = leptos::view! { <ProjectCard project task_counts=counts /> }.to_html();
        assert!(html.contains("0 tasks"));
        assert!(!html.contains("pending"));
        assert!(!html.contains("in progress"));
        assert!(!html.contains("in review"));
        assert!(!html.contains("completed"));
    }
}
