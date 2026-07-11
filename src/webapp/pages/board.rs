use leptos::prelude::*;

use crate::db::schema::{Project, Task};
use crate::webapp::components::task_card::TaskCard;

fn tasks_for_status(tasks: &[Task], status: &str) -> Vec<Task> {
    tasks
        .iter()
        .filter(|t| t.status == status)
        .cloned()
        .collect()
}

#[component]
pub fn BoardPage(project: Project, tasks: Vec<Task>) -> impl IntoView {
    let pending = tasks_for_status(&tasks, "pending");
    let in_progress = tasks_for_status(&tasks, "in_progress");
    let in_review = tasks_for_status(&tasks, "in_review");
    let completed = tasks_for_status(&tasks, "completed");

    let render_column = |_status: &str, label: &str, color_class: &str, items: Vec<Task>| {
        view! {
            <div class="column">
                <div class={format!("box {}", color_class)}>
                    <h3 class="title is-5">{format!("{} ({})", label, items.len())}</h3>
                </div>
                {if items.is_empty() {
                    view! { <p class="has-text-grey is-size-7" style="padding: 0.5rem;">"No tasks"</p> }.into_any()
                } else {
                    view! {
                        {items.into_iter().map(|task| {
                            view! { <TaskCard task /> }
                        }).collect::<Vec<_>>()}
                    }.into_any()
                }}
            </div>
        }
    };

    view! {
        <section class="section">
            <nav class="breadcrumb" aria-label="breadcrumbs">
                <ul>
                    <li><a href="/webapp">"Dashboard"</a></li>
                    <li class="is-active"><a href="#">{project.name.clone()}</a></li>
                </ul>
            </nav>

            <div class="level">
                <div class="level-left">
                    <h1 class="title">{project.name.clone()}</h1>
                </div>
                <div class="level-right">
                    <button id="new-task-btn" class="button is-primary">
                        <span class="icon is-small"><i class="mdi mdi-plus"></i></span>
                        <span>"New Task"</span>
                    </button>
                </div>
            </div>

            <div id="new-task-form" class="box is-hidden">
                <form id="create-task-form">
                    <div class="field">
                        <label class="label">"Task Title"</label>
                        <div class="control">
                            <input name="title" class="input" type="text" placeholder="Task title" required />
                        </div>
                    </div>
                    <div class="field">
                        <label class="label">"Description"</label>
                        <div class="control">
                            <textarea name="original_request" class="textarea" placeholder="Describe the task..."></textarea>
                        </div>
                    </div>
                    <div class="field">
                        <div class="control">
                            <button type="submit" class="button is-success">"Create Task"</button>
                            <button type="button" id="cancel-task-btn" class="button is-light">"Cancel"</button>
                        </div>
                    </div>
                </form>
            </div>

            <div class="columns">
                {render_column("pending", "Pending", "has-background-grey-lighter", pending)}
                {render_column("in_progress", "In Progress", "has-background-info-light", in_progress)}
                {render_column("in_review", "In Review", "has-background-warning-light", in_review)}
                {render_column("completed", "Completed", "has-background-success-light", completed)}
            </div>
        </section>
        <script>
            {r#"document.addEventListener('DOMContentLoaded',function(){
                var newBtn=document.getElementById('new-task-btn');
                var form=document.getElementById('new-task-form');
                var cancelBtn=document.getElementById('cancel-task-btn');
                if(!newBtn||!form)return;
                newBtn.addEventListener('click',function(){form.classList.toggle('is-hidden');});
                if(cancelBtn)cancelBtn.addEventListener('click',function(){form.classList.add('is-hidden');});
                var createForm=document.getElementById('create-task-form');
                if(createForm)createForm.addEventListener('submit',function(ev){
                    ev.preventDefault();
                    var projectId=parseInt(window.location.pathname.split('/').pop(),10);
                    var data={
                        project_id: projectId,
                        title: createForm.querySelector('[name=title]').value,
                        original_request: createForm.querySelector('[name=original_request]').value
                    };
                    apiCall('/api/tasks',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(data)})
                        .then(function(r){if(r.ok)window.location.reload();});
                });
            });"#}
        </script>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDateTime;

    fn make_project() -> Project {
        Project {
            id: 1,
            user_id: uuid::Uuid::new_v4(),
            name: "Test Project".into(),
            repo_folder_path: "/tmp/repo".into(),
            subproject_path: None,
            created_at: NaiveDateTime::parse_from_str("2024-01-15 10:00:00", "%Y-%m-%d %H:%M:%S")
                .unwrap(),
        }
    }

    fn make_task(status: &str) -> Task {
        Task {
            id: 1,
            project_id: 1,
            user_id: uuid::Uuid::new_v4(),
            title: format!("Task-{}", status),
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
    fn test_board_renders_four_columns() {
        let project = make_project();
        let tasks = vec![];
        let html = leptos::view! { <BoardPage project tasks /> }.to_html();
        assert!(html.contains("Pending"));
        assert!(html.contains("In Progress"));
        assert!(html.contains("In Review"));
        assert!(html.contains("Completed"));
        assert!(html.contains("Dashboard"));
    }

    #[test]
    fn test_board_tasks_grouped_correctly() {
        let project = make_project();
        let tasks = vec![
            make_task("pending"),
            make_task("pending"),
            make_task("in_progress"),
            make_task("completed"),
        ];
        let html = leptos::view! { <BoardPage project tasks /> }.to_html();
        assert!(html.contains("Pending (2)"));
        assert!(html.contains("In Progress (1)"));
        assert!(html.contains("Completed (1)"));
        assert!(html.contains("In Review (0)"));
        assert!(html.contains("No tasks"));
    }

    #[test]
    fn test_board_has_new_task_button() {
        let project = make_project();
        let tasks = vec![];
        let html = leptos::view! { <BoardPage project tasks /> }.to_html();
        assert!(html.contains("New Task"));
        assert!(html.contains("mdi-plus"));
    }
}
