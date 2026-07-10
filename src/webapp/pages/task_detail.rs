use leptos::prelude::*;

use crate::db::schema::{AgentType, RunStatus, Task, TaskAgentRun};
use crate::providers::registry::AgentConfigStatus;
use crate::webapp::components::markdown_viewer::MarkdownViewer;

fn agent_button_state(
    agent_type: &AgentType,
    task: &Task,
    agent_runs: &[TaskAgentRun],
) -> (&'static str, &'static str, bool) {
    if let Some(run) = agent_runs.iter().find(|r| r.agent_type == *agent_type) {
        match run.status {
            RunStatus::Running => ("Running", "is-info is-loading", true),
            RunStatus::Completed => ("Completed", "is-success", true),
            RunStatus::Failed => ("Failed - Retry", "is-danger", false),
            RunStatus::Blocked => ("Blocked", "is-warning", true),
            RunStatus::Pending => ("Pending", "is-light", true),
        }
    } else {
        match agent_type {
            AgentType::Planification if task.planification_complete => {
                ("Completed", "is-success", true)
            }
            _ => ("Run", "is-primary", false),
        }
    }
}

fn agent_type_label(agent_type: &AgentType) -> &'static str {
    match agent_type {
        AgentType::Planification => "Planification",
        AgentType::Implementation => "Implementation",
        AgentType::Refinement => "Refinement",
        AgentType::Review => "Review",
        AgentType::Pr => "PR",
        AgentType::Yolo => "Yolo",
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

fn run_status_class(status: &RunStatus) -> &'static str {
    match status {
        RunStatus::Pending => "is-light",
        RunStatus::Running => "is-info",
        RunStatus::Completed => "is-success",
        RunStatus::Failed => "is-danger",
        RunStatus::Blocked => "is-warning",
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

fn status_class(status: &str) -> &'static str {
    match status {
        "pending" => "is-light",
        "in_progress" => "is-info",
        "in_review" => "is-warning",
        "completed" => "is-success",
        _ => "is-light",
    }
}

#[component]
pub fn TaskDetailPage(
    task: Task,
    doc_content: Option<String>,
    agent_runs: Vec<TaskAgentRun>,
    agent_config_statuses: Vec<AgentConfigStatus>,
) -> impl IntoView {
    let status_badge_class = status_class(&task.status);
    let status_label_str = status_label(&task.status);
    let task_id = task.id.to_string();

    let agent_types = [
        AgentType::Planification,
        AgentType::Implementation,
        AgentType::Refinement,
        AgentType::Review,
        AgentType::Pr,
    ];

    view! {
        <section class="section">
            <nav class="breadcrumb" aria-label="breadcrumbs">
                <ul>
                    <li><a href="/webapp">"Dashboard"</a></li>
                    <li><a href={format!("/webapp/projects/{}", task.project_id)}>"Board"</a></li>
                    <li class="is-active"><a href="#">{task.title.clone()}</a></li>
                </ul>
            </nav>

            <div class="level">
                <div class="level-left">
                    <h1 class="title">{task.title.clone()}</h1>
                    <span class={format!("tag {} ml-2", status_badge_class)}>{status_label_str}</span>
                </div>
            </div>

            <div class="columns">
                <div class="column is-two-thirds">
                    <div class="box">
                        <h2 class="title is-4">"Documentation"</h2>
                        {if let Some(ref doc) = doc_content {
                            if doc.is_empty() {
                                view! {
                                    <p class="has-text-grey">"No document yet. Start by running the Planification agent."</p>
                                }.into_any()
                            } else {
                                view! { <MarkdownViewer content=doc.clone() /> }.into_any()
                            }
                        } else {
                            view! {
                                <p class="has-text-grey">"No document yet. Start by running the Planification agent."</p>
                            }.into_any()
                        }}
                    </div>
                </div>

                <div class="column is-one-third">
                    <div class="box">
                        <h2 class="title is-4">"Agents"</h2>
                        {agent_types.iter().map(|agent_type| {
                            let (btn_label, btn_class, btn_disabled) =
                                agent_button_state(agent_type, &task, &agent_runs);
                            let agent_val = agent_type_value(agent_type);
                            let agent_label = agent_type_label(agent_type);
                            let agent_val_str = agent_val;
                            let status = agent_config_statuses.iter().find(|s| s.agent_type == agent_val_str);
                            view! {
                                <div class="box" style="padding:0.75rem">
                                    <div style="display:flex;justify-content:space-between;align-items:center;margin-bottom:0.5rem">
                                        <strong>{agent_label}</strong>
                                        {if let Some(s) = status.filter(|s| s.configured) {
                                            view! {
                                                <span class="tag is-success">
                                                    {format!("{} ({})",
                                                        s.label.as_deref().unwrap_or("config"),
                                                        s.scope.as_deref().unwrap_or("?"))}
                                                </span>
                                            }.into_any()
                                        } else {
                                            view! {
                                                <span class="tag is-danger">"No Agent Config"</span>
                                            }.into_any()
                                        }}
                                    </div>
                                    <button
                                        class={format!("button is-fullwidth {}", btn_class)}
                                        data-task-id={task_id.clone()}
                                        data-agent-type={agent_val}
                                        disabled=btn_disabled
                                    >
                                        {btn_label}
                                    </button>
                                </div>
                            }
                        }).collect::<Vec<_>>()}
                    </div>

                    <div class="box">
                        <h2 class="title is-4">"Actions"</h2>
                        <a
                            class="button is-fullwidth is-info mb-2"
                            href={format!("/webapp/projects/{}/tasks/{}/chat", task.project_id, task.id)}
                        >
                            "Chat"
                        </a>
                    </div>

                    <div class="box">
                        <h2 class="title is-4">"Run History"</h2>
                        {if agent_runs.is_empty() {
                            view! { <p class="has-text-grey is-size-7">"No runs yet."</p> }.into_any()
                        } else {
                            view! {
                                <table class="table is-fullwidth is-striped is-hoverable">
                                    <thead>
                                        <tr>
                                            <th>"Agent"</th>
                                            <th>"Status"</th>
                                            <th>"Date"</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        {agent_runs.iter().map(|run| {
                                            let run_status_class = run_status_class(&run.status);
                                            let created = run.created_at.format("%Y-%m-%d %H:%M").to_string();
                                            view! {
                                                <tr>
                                                    <td>{agent_type_label(&run.agent_type)}</td>
                                                    <td><span class={format!("tag {}", run_status_class)}>{run.status.to_string()}</span></td>
                                                    <td><small class="has-text-grey">{created}</small></td>
                                                </tr>
                                            }
                                        }).collect::<Vec<_>>()}
                                    </tbody>
                                </table>
                            }.into_any()
                        }}
                    </div>
                </div>
            </div>
        </section>
        <script>
            {r#"document.addEventListener('DOMContentLoaded',function(){
                var buttons=document.querySelectorAll('[data-task-id][data-agent-type]');
                buttons.forEach(function(btn){
                    btn.addEventListener('click',function(){
                        var taskId=btn.getAttribute('data-task-id');
                        var agentType=btn.getAttribute('data-agent-type');
                        btn.disabled=true;
                        btn.classList.add('is-loading');
                        apiCall('/api/tasks/'+taskId+'/agent-runs',{
                            method:'POST',
                            headers:{'Content-Type':'application/json'},
                            body:JSON.stringify({agent_type:agentType})
                        }).then(function(r){
                            if(r.status===409){showMessage('Agent already running for this task');}
                            else if(r.status===403){showMessage('Provider credentials missing');}
                            else if(r.ok){window.location.reload();}
                            else{showMessage('Error starting agent');}
                        }).finally(function(){
                            btn.disabled=false;
                            btn.classList.remove('is-loading');
                        });
                    });
                });
                function showMessage(msg){
                    var existing=document.getElementById('agent-message');
                    if(existing)existing.remove();
                    var div=document.createElement('div');
                    div.id='agent-message';
                    div.className='notification is-warning';
                    div.style='position:fixed;top:4rem;right:1rem;z-index:9999;';
                    div.textContent=msg;
                    document.body.appendChild(div);
                    setTimeout(function(){div.remove();},5000);
                }
            });"#}
        </script>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDateTime;

    fn empty_statuses() -> Vec<AgentConfigStatus> {
        vec![]
    }

    fn make_task() -> Task {
        Task {
            id: 1,
            project_id: 1,
            user_id: uuid::Uuid::new_v4(),
            title: "Implement feature X".into(),
            status: "in_progress".into(),
            workflow_complete: false,
            workflow_blocked: false,
            workflow_run_count: 1,
            planification_complete: true,
            pr_agent_complete: false,
            refinement_complete: false,
            yolo_mode: false,
            created_at: NaiveDateTime::parse_from_str("2024-06-01 12:00:00", "%Y-%m-%d %H:%M:%S")
                .unwrap(),
        }
    }

    fn make_run(agent_type: AgentType, status: RunStatus) -> TaskAgentRun {
        TaskAgentRun {
            id: uuid::Uuid::new_v4(),
            task_id: 1,
            agent_type,
            status,
            conversation_id: None,
            created_at: NaiveDateTime::parse_from_str("2024-06-01 12:00:00", "%Y-%m-%d %H:%M:%S")
                .unwrap(),
            completed_at: None,
        }
    }

    #[test]
    fn test_task_detail_has_run_buttons() {
        let task = make_task();
        let agent_runs = vec![];
        let doc_content = Some("Hello".into());
        let statuses = empty_statuses();
        let html = leptos::view! { <TaskDetailPage task doc_content agent_runs agent_config_statuses=statuses /> }.to_html();
        assert!(html.contains("Planification"));
        assert!(html.contains("Implementation"));
        assert!(html.contains("Refinement"));
        assert!(html.contains("Review"));
        assert!(html.contains("PR"));
        assert!(html.contains("Run"));
    }

    #[test]
    fn test_task_detail_renders_markdown_section() {
        let task = make_task();
        let agent_runs = vec![];
        let doc_content = Some("# Hello World".into());
        let statuses = empty_statuses();
        let html = leptos::view! { <TaskDetailPage task doc_content agent_runs agent_config_statuses=statuses /> }.to_html();
        assert!(html.contains("Documentation"));
        assert!(html.contains("<h1>"));
        assert!(html.contains("Hello World"));
        assert!(html.contains("class=\"content\""));
    }

    #[test]
    fn test_task_detail_shows_run_history() {
        let task = make_task();
        let agent_runs = vec![
            make_run(AgentType::Planification, RunStatus::Completed),
            make_run(AgentType::Implementation, RunStatus::Running),
        ];
        let doc_content = None;
        let statuses = empty_statuses();
        let html = leptos::view! { <TaskDetailPage task doc_content agent_runs agent_config_statuses=statuses /> }.to_html();
        assert!(html.contains("Run History"));
        assert!(html.contains("Planification"));
        assert!(html.contains("completed"));
        assert!(html.contains("running"));
    }

    #[test]
    fn test_task_detail_empty_doc_shows_prompt() {
        let task = make_task();
        let agent_runs = vec![];
        let doc_content = None;
        let statuses = empty_statuses();
        let html = leptos::view! { <TaskDetailPage task doc_content agent_runs agent_config_statuses=statuses /> }.to_html();
        assert!(html.contains("No document yet"));
    }

    #[test]
    fn test_task_detail_no_runs_shows_message() {
        let task = make_task();
        let agent_runs = vec![];
        let doc_content = None;
        let statuses = empty_statuses();
        let html = leptos::view! { <TaskDetailPage task doc_content agent_runs agent_config_statuses=statuses /> }.to_html();
        assert!(html.contains("No runs yet"));
    }

    #[test]
    fn test_task_detail_shows_config_status() {
        let task = make_task();
        let agent_runs = vec![];
        let doc_content = None;
        let statuses = vec![
            AgentConfigStatus {
                agent_type: "planification".into(),
                configured: true,
                scope: Some("User".into()),
                label: Some("gpt-4".into()),
            },
            AgentConfigStatus {
                agent_type: "implementation".into(),
                configured: false,
                scope: None,
                label: None,
            },
        ];
        let html = leptos::view! { <TaskDetailPage task doc_content agent_runs agent_config_statuses=statuses /> }.to_html();
        assert!(html.contains("gpt-4"));
        assert!(html.contains("is-success"));
        assert!(html.contains("is-danger"));
        assert!(html.contains("No Agent Config"));
    }
}
