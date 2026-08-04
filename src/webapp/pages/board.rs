use leptos::prelude::*;

use crate::db::schema::Project;
use crate::webapp::components::task_card::{TaskCard, TaskCardData};

fn tasks_for_status(tasks: &[TaskCardData], status: &str) -> Vec<TaskCardData> {
    tasks
        .iter()
        .filter(|t| t.task.status == status)
        .cloned()
        .collect()
}

#[component]
pub fn BoardPage(project: Project, tasks: Vec<TaskCardData>) -> impl IntoView {
    let pending = tasks_for_status(&tasks, "pending");
    let in_progress = tasks_for_status(&tasks, "in_progress");
    let in_review = tasks_for_status(&tasks, "in_review");
    let completed = tasks_for_status(&tasks, "completed");

    let render_column = |label: &str, color_class: &str, items: Vec<TaskCardData>, status: &str| {
        view! {
            <div class="column is-one-quarter" data-status={status.to_string()}>
                <div class={format!("box {}", color_class)}>
                    <h3 class="title is-5">{format!("{} ({})", label, items.len())}</h3>
                </div>
                {if items.is_empty() {
                    view! { <p class="has-text-grey is-size-7" style="padding: 0.5rem;">"No tasks"</p> }.into_any()
                } else {
                    view! {
                        {items.into_iter().map(|td| {
                            view! { <TaskCard data=td /> }
                        }).collect::<Vec<_>>()}
                    }.into_any()
                }}
            </div>
        }
    };

    view! {
        <section class="section">
            <div class="level">
                <div class="level-left">
                    <h1 class="title">{project.name.clone()}</h1>
                </div>
                <div class="level-right">
                    <button id="new-task-btn" class="button is-small is-primary">
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
                    <div class="field is-grouped is-grouped-right">
                        <div class="control">
                            <button type="submit" class="button is-small is-success">"Create Task"</button>
                        </div>
                        <div class="control">
                            <button type="button" id="cancel-task-btn" class="button is-small is-light">"Cancel"</button>
                        </div>
                    </div>
                </form>
            </div>

            <div class="columns">
                {render_column("Pending", "has-background-grey-lighter", pending, "pending")}
                {render_column("In Progress", "has-background-info-light", in_progress, "in_progress")}
                {render_column("In Review", "has-background-warning-light", in_review, "in_review")}
                {render_column("Completed", "has-background-success-light", completed, "completed")}
            </div>
        </section>
        <script src="/webapp/assets/dragula.min.js"></script>
        <script>
            {r#"document.addEventListener('DOMContentLoaded',function(){
                // Delete task from card
                document.addEventListener('click',function(e){
                    var delBtn=e.target.closest('[data-task-delete]');
                    if(!delBtn)return;
                    e.preventDefault();
                    e.stopPropagation();
                    var taskId=delBtn.getAttribute('data-task-id');
                    if(!confirm('Are you sure you want to delete this task?'))return;
                    apiCall('/api/tasks/'+taskId,{method:'DELETE'})
                        .then(function(r){if(r.ok)window.location.reload();});
                });
                // New task form
                var newBtn=document.getElementById('new-task-btn');
                var form=document.getElementById('new-task-form');
                var cancelBtn=document.getElementById('cancel-task-btn');
                if(newBtn&&form){
                    newBtn.addEventListener('click',function(){form.classList.toggle('is-hidden');});
                    if(cancelBtn)cancelBtn.addEventListener('click',function(){form.classList.add('is-hidden');});
                }
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
                // Drag-and-drop task cards via dragula
                var containers=Array.from(document.querySelectorAll('.columns>.column[data-status]'));
                var drake=dragula(containers,{
                    moves:function(el,source,handle,sibling){
                        return el.tagName==='A'&&el.classList.contains('card')&&!handle.closest('[data-task-delete]');
                    }
                });
                drake.on('drop',function(el,target,source,sibling){
                    if(target===source)return;
                    var taskId=el.getAttribute('data-task-id');
                    if(!taskId)return;
                    var newStatus=target.getAttribute('data-status');
                    function revertDrag(){
                        source.appendChild(el);
                        updateColumnCount(target);
                        updateColumnCount(source);
                    }
                    apiCall('/api/tasks/'+taskId,{
                        method:'PUT',
                        headers:{'Content-Type':'application/json'},
                        body:JSON.stringify({status:newStatus})
                    }).then(function(r){
                        if(r.ok){
                            updateColumnCount(target);
                            updateColumnCount(source);
                        }else{
                            revertDrag();
                        }
                    }).catch(revertDrag);
                });
                function updateColumnCount(col){
                    var cards=col.querySelectorAll('a.card');
                    var count=cards.length;
                    var title=col.querySelector('h3.title');
                    if(title){
                        var label=title.textContent.split(' (')[0];
                        title.textContent=label+' ('+count+')';
                    }
                    var noTasks=col.querySelector('p.has-text-grey');
                    if(count===0){
                        if(!noTasks){
                            var header=col.querySelector('div.box');
                            if(header){
                                var p=document.createElement('p');
                                p.className='has-text-grey is-size-7';
                                p.style.padding='0.5rem';
                                p.textContent='No tasks';
                                header.insertAdjacentElement('afterend',p);
                            }
                        }
                    }else{
                        if(noTasks)noTasks.remove();
                    }
                }
            });"#}
        </script>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::schema::Task;
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
            workflow_blocked: false,
            workflow_run_count: 0,
            yolo_mode: false,
            created_at: NaiveDateTime::parse_from_str("2024-06-01 12:00:00", "%Y-%m-%d %H:%M:%S")
                .unwrap(),
        }
    }

    fn make_data(status: &str) -> TaskCardData {
        TaskCardData {
            task: make_task(status),
            agent_types_run: vec![],
            running_agent: None,
        }
    }

    #[test]
    fn test_board_renders_four_columns() {
        let project = make_project();
        let project_name = project.name.clone();
        let tasks = vec![];
        let html = leptos::view! { <BoardPage project tasks /> }.to_html();
        assert!(html.contains("Pending"));
        assert!(html.contains("In Progress"));
        assert!(html.contains("In Review"));
        assert!(html.contains("Completed"));
        assert!(html.contains(project_name.as_str()));
    }

    #[test]
    fn test_board_tasks_grouped_correctly() {
        let project = make_project();
        let tasks = vec![
            make_data("pending"),
            make_data("pending"),
            make_data("in_progress"),
            make_data("completed"),
        ];
        let html = leptos::view! { <BoardPage project tasks /> }.to_html();
        assert!(html.contains("Pending (2)"));
        assert!(html.contains("In Progress (1)"));
        assert!(html.contains("Completed (1)"));
        assert!(html.contains("In Review (0)"));
        assert!(html.contains("No tasks"));
    }

    #[test]
    fn test_board_buttons_have_is_small() {
        let project = make_project();
        let tasks = vec![];
        let html = leptos::view! { <BoardPage project tasks /> }.to_html();
        assert!(
            html.contains(r#"class="button is-small is-primary""#),
            "New Task button should have is-small"
        );
        assert!(
            html.contains(r#"class="button is-small is-success""#),
            "Create Task button should have is-small"
        );
        assert!(
            html.contains(r#"class="button is-small is-light""#),
            "Cancel button should have is-small"
        );
    }

    #[test]
    fn test_board_create_task_buttons_in_right_aligned_group() {
        let project = make_project();
        let tasks = vec![];
        let html = leptos::view! { <BoardPage project tasks /> }.to_html();
        assert!(
            html.contains(r#"class="field is-grouped is-grouped-right""#),
            "Create Task / Cancel should be in a right-aligned button group"
        );
        assert!(
            html.contains(r#">Create Task</button></div>"#),
            "Create Task should be wrapped in a control"
        );
        assert!(
            html.contains(r#">Cancel</button></div>"#),
            "Cancel should be wrapped in a control"
        );
    }

    #[test]
    fn test_board_has_new_task_button() {
        let project = make_project();
        let tasks = vec![];
        let html = leptos::view! { <BoardPage project tasks /> }.to_html();
        assert!(html.contains("New Task"));
        assert!(html.contains("mdi-plus"));
    }

    #[test]
    fn test_board_columns_have_data_status() {
        let project = make_project();
        let tasks = vec![];
        let html = leptos::view! { <BoardPage project tasks /> }.to_html();
        assert!(html.contains(r#"data-status="pending""#));
        assert!(html.contains(r#"data-status="in_progress""#));
        assert!(html.contains(r#"data-status="in_review""#));
        assert!(html.contains(r#"data-status="completed""#));
    }
}
