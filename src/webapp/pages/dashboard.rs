use std::collections::HashMap;

use leptos::prelude::*;
use uuid::Uuid;

use crate::db::schema::Project;
use crate::webapp::components::project_card::{ProjectCard, TaskCounts};

#[component]
pub fn DashboardPage(
    projects: Vec<Project>,
    task_counts: HashMap<Uuid, TaskCounts>,
) -> impl IntoView {
    view! {
        <section class="section">
            <div class="level">
                <div class="level-left">
                    <h1 class="title">"Projects"</h1>
                </div>
                <div class="level-right">
                    <button id="new-project-btn" class="button is-primary">
                        <span class="icon is-small"><i class="mdi mdi-plus"></i></span>
                        <span>"New Project"</span>
                    </button>
                </div>
            </div>

            <div id="new-project-form" class="box is-hidden">
                <form id="create-project-form">
                    <div class="field">
                        <label class="label">"Project Name"</label>
                        <div class="control">
                            <input name="name" class="input" type="text" placeholder="My Project" required />
                        </div>
                    </div>
                    <div class="field">
                        <label class="label">"Repo Folder Path"</label>
                        <div class="control">
                            <input name="repo_folder_path" class="input" type="text" placeholder="/path/to/repo" required />
                        </div>
                    </div>
                    <div class="field">
                        <label class="label">"Subproject Path (optional)"</label>
                        <div class="control">
                            <input name="subproject_path" class="input" type="text" placeholder="subdir" />
                        </div>
                    </div>
                    <div class="field">
                        <div class="control">
                            <button type="submit" class="button is-success">"Create Project"</button>
                            <button type="button" id="cancel-project-btn" class="button is-light">"Cancel"</button>
                        </div>
                    </div>
                </form>
            </div>

            {if projects.is_empty() {
                view! {
                    <div class="box">
                        <p class="has-text-grey">"No projects yet. Create one to get started."</p>
                    </div>
                }.into_any()
            } else {
                view! {
                    <div class="columns is-multiline">
                        {projects.into_iter().map(|project| {
                            let counts = task_counts.get(&project.id).cloned().unwrap_or_default();
                            view! {
                                <div class="column is-one-third">
                                    <ProjectCard project task_counts=counts />
                                </div>
                            }
                        }).collect::<Vec<_>>()}
                    </div>
                }.into_any()
            }}
        </section>
        <script>
            {r#"document.addEventListener('DOMContentLoaded',function(){
                var newBtn=document.getElementById('new-project-btn');
                var form=document.getElementById('new-project-form');
                var cancelBtn=document.getElementById('cancel-project-btn');
                if(!newBtn||!form)return;
                newBtn.addEventListener('click',function(){form.classList.toggle('is-hidden');});
                if(cancelBtn)cancelBtn.addEventListener('click',function(){form.classList.add('is-hidden');});
                var createForm=document.getElementById('create-project-form');
                if(createForm)createForm.addEventListener('submit',function(ev){
                    ev.preventDefault();
                    var data={
                        name: createForm.querySelector('[name=name]').value,
                        repo_folder_path: createForm.querySelector('[name=repo_folder_path]').value,
                        subproject_path: createForm.querySelector('[name=subproject_path]').value || null
                    };
                    apiCall('/api/projects',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(data)})
                        .then(function(r){if(r.ok)window.location.reload();});
                });
            });"#}
        </script>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dashboard_shows_new_project_button() {
        let projects = vec![];
        let task_counts = HashMap::new();
        let html = leptos::view! { <DashboardPage projects task_counts /> }.to_html();
        assert!(html.contains("New Project"));
        assert!(html.contains("Projects"));
        assert!(html.contains("No projects yet"));
    }

    #[test]
    fn test_dashboard_shows_project_list() {
        let project = Project {
            id: Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap(),
            user_id: Uuid::new_v4(),
            name: "Alpha".into(),
            repo_folder_path: "/tmp/alpha".into(),
            subproject_path: None,
            created_at: chrono::NaiveDateTime::parse_from_str(
                "2024-01-15 10:00:00",
                "%Y-%m-%d %H:%M:%S",
            )
            .unwrap(),
        };
        let mut task_counts = HashMap::new();
        task_counts.insert(
            project.id,
            TaskCounts {
                pending: 2,
                in_progress: 0,
                in_review: 0,
                completed: 1,
            },
        );
        let html = leptos::view! { <DashboardPage projects=vec![project] task_counts /> }.to_html();
        assert!(html.contains("Alpha"));
        assert!(!html.contains("No projects yet"));
    }
}
