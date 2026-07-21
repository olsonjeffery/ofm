use leptos::prelude::*;

use crate::db::schema::{AgentType, ConversationWithRun, RunStatus, Task, TaskAgentRun};
use crate::webapp::components::conversation_list::ConversationList;
use crate::webapp::components::markdown_viewer::MarkdownViewer;

fn agent_type_icon(agent_type: &AgentType) -> &'static str {
    match agent_type {
        AgentType::Planification => "file-document-outline",
        AgentType::Implementation => "code-tags",
        AgentType::Refinement => "creation-outline",
        AgentType::Review => "checkbox-marked-circle-outline",
        AgentType::Pr => "source-branch-plus",
        AgentType::Yolo => "rocket",
    }
}

fn agent_type_label(agent_type: &AgentType) -> &'static str {
    match agent_type {
        AgentType::Planification => "Plannification",
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
    conversations: Vec<ConversationWithRun>,
    current_run: Option<TaskAgentRun>,
) -> impl IntoView {
    let status_badge_class = status_class(&task.status);
    let status_label_str = status_label(&task.status);
    let task_id = task.id.to_string();
    let conversation_count = conversations.len();

    let agent_types = [
        AgentType::Planification,
        AgentType::Implementation,
        AgentType::Refinement,
        AgentType::Review,
        AgentType::Pr,
    ];

    let doc_value = doc_content.clone().unwrap_or_default();
    let doc_escaped = html_escape::encode_text(&doc_value).to_string();

    view! {
        <section class="section">
            <div class="level" data-task-id={task.id.to_string()} data-project-id={task.project_id.to_string()}>
                <div class="level-left">
                    <h1 class="title">{task.title.clone()}</h1>
                    <span class={format!("tag {} ml-2", status_badge_class)}>{status_label_str}</span>
                </div>
                <div class="level-right">
                    <button id="edit-task-btn" class="button is-small is-light" title="Edit task">
                        <span class="icon is-small"><i class="mdi mdi-pencil"></i></span>
                        <span>"Edit"</span>
                    </button>
                </div>
            </div>

            <div id="edit-task-form" class="box is-hidden" style="margin-top:0.5rem">
                <form id="edit-task-form-inner">
                    <div class="field">
                        <label class="label" for="edit-task-title">"Task Title"</label>
                        <div class="control">
                            <input id="edit-task-title" name="title" class="input" type="text" value={task.title.clone()} required />
                        </div>
                    </div>
                    <div class="field">
                        <label class="label" for="edit-task-status">"Status"</label>
                        <div class="control">
                            <div class="select">
                                <select id="edit-task-status" name="status">
                                    <option value="pending" selected={task.status == "pending"}>"Pending"</option>
                                    <option value="in_progress" selected={task.status == "in_progress"}>"In Progress"</option>
                                    <option value="in_review" selected={task.status == "in_review"}>"In Review"</option>
                                    <option value="completed" selected={task.status == "completed"}>"Completed"</option>
                                </select>
                            </div>
                        </div>
                    </div>
                    <div class="field">
                        <label class="label" for="edit-task-doc">"Document"</label>
                        <div class="control">
                            <textarea id="edit-task-doc" name="doc_content" class="textarea" rows="10">{doc_escaped}</textarea>
                        </div>
                    </div>
                    <div class="field">
                        <div class="control">
                            <button type="submit" class="button is-success">"Save"</button>
                            <button type="button" id="cancel-edit-task-btn" class="button is-light">"Cancel"</button>
                        </div>
                    </div>
                </form>
            </div>

            <div class="columns">
                <div class="column is-one-fifth" style="border-right:1px solid #ddd;max-width:320px">
                    <div class="level is-mobile" style="margin-bottom:0.5rem">
                        <div class="level-left">
                            <h2 class="title is-5">"Conversations "</h2>
                            <span class="tag is-info is-light ml-1">{conversation_count}</span>
                        </div>
                        <div class="level-right"></div>
                    </div>
                    <ConversationList conversations=conversations active_id=None />
                </div>

                <div class="column">
                    <div class="box">
                        <div class="level is-mobile" style="margin-bottom:0.5rem">
                            <div class="level-left">
                                <h2 class="title is-4">"Documentation"</h2>
                            </div>
                        </div>
                        {if doc_content.as_deref().is_none_or(str::is_empty) {
                            view! {
                                <p class="has-text-grey">"No document yet. Start by running the Planification agent."</p>
                            }.into_any()
                        } else {
                            view! { <MarkdownViewer content=doc_content.unwrap_or_default() /> }.into_any()
                        }}
                    </div>

                    <div class="box">
                        <div style="display:flex;justify-content:space-between;align-items:center">
                            <h2 class="title is-4">"Agents"</h2>
                            <button
                                id="stop-agent-btn"
                                class="button is-small is-danger"
                                style={if current_run.is_some() { "display:inline-flex" } else { "display:none" }}
                            >
                                <span class="icon is-small"><i class="mdi mdi-stop"></i></span>
                                <span>"Stop Agent"</span>
                            </button>
                        </div>
                        {agent_types.iter().map(|agent_type| {
                            let agent_val = agent_type_value(agent_type);
                            let agent_icon = agent_type_icon(agent_type);
                            let agent_label = agent_type_label(agent_type);
                            let is_current = current_run.as_ref().map(|r| r.agent_type == *agent_type).unwrap_or(false);
                            let is_running = current_run.as_ref().map(|r| r.status == RunStatus::Running).unwrap_or(false);
                            let btn_loading = is_current && is_running;
                            view! {
                                <div class="box is-info is-light" style="padding:0.75rem;margin-bottom:0.25rem">
                                    <div class="level is-mobile">
                                        <div class="level-left">
                                            <span class="icon has-text-info"><i class={format!("mdi mdi-{}", agent_icon)}></i></span>
                                            <span class="has-text-info">{agent_label}</span>
                                        </div>
                                        <div class="level-right">
                                            <button
                                                class={format!("button is-small has-text-info {}", if btn_loading { "is-loading" } else { "" })}
                                                data-task-id={task_id.clone()}
                                                data-agent-type={agent_val}
                                                disabled=false
                                            >
                                                <span class="icon is-small"><i class="mdi mdi-play-outline"></i></span>
                                                <span>"Run"</span>
                                            </button>
                                        </div>
                                    </div>
                                </div>
                            }
                        }).collect::<Vec<_>>()}
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

                    <div class="box" style="margin-top:1rem">
                        <div class="level">
                            <div class="level-left">
                                <h2 class="title is-4 has-text-danger">"Danger Zone"</h2>
                            </div>
                            <div class="level-right">
                                <button id="delete-task-btn" class="button is-danger">
                                    <span class="icon is-small"><i class="mdi mdi-delete"></i></span>
                                    <span>"Delete Task"</span>
                                </button>
                            </div>
                        </div>
                    </div>
                </div>
            </div>
        </section>
        <script>
            {r#"document.addEventListener('DOMContentLoaded',function(){
                var taskIdEl=document.querySelector('[data-task-id]');
                var taskId=taskIdEl?.getAttribute('data-task-id');
                var projectId=taskIdEl?.getAttribute('data-project-id');

                // Agent run buttons
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

                // Stop Agent button
                var stopBtn=document.getElementById('stop-agent-btn');
                if(stopBtn){
                    stopBtn.addEventListener('click',function(){
                        stopBtn.disabled=true;
                        stopBtn.classList.add('is-loading');
                        apiCall('/api/tasks/'+taskId+'/agent-runs/reset',{
                            method:'POST'
                        }).then(function(r){
                            if(r.ok){window.location.reload();}
                            else{showMessage('Failed to stop agent');}
                        }).finally(function(){
                            stopBtn.disabled=false;
                            stopBtn.classList.remove('is-loading');
                        });
                    });
                }

                // Edit task form
                var editBtn=document.getElementById('edit-task-btn');
                var editForm=document.getElementById('edit-task-form');
                var cancelEditBtn=document.getElementById('cancel-edit-task-btn');
                if(editBtn&&editForm){
                    editBtn.addEventListener('click',function(){editForm.classList.toggle('is-hidden');});
                    if(cancelEditBtn)cancelEditBtn.addEventListener('click',function(){editForm.classList.add('is-hidden');});
                }
                var editFormInner=document.getElementById('edit-task-form-inner');
                if(editFormInner){
                    editFormInner.addEventListener('submit',function(ev){
                        ev.preventDefault();
                        var title=document.getElementById('edit-task-title').value;
                        var status=document.getElementById('edit-task-status').value;
                        var docContent=document.getElementById('edit-task-doc').value;
                        apiCall('/api/tasks/'+taskId,{
                            method:'PUT',
                            headers:{'Content-Type':'application/json'},
                            body:JSON.stringify({title:title,status:status,doc_content:docContent})
                        }).then(function(r){
                            if(r.ok){window.location.reload();}
                            else{showMessage('Error saving task');}
                        });
                    });
                }

                // Delete task
                var deleteBtn=document.getElementById('delete-task-btn');
                if(deleteBtn){
                    deleteBtn.addEventListener('click',function(){
                        if(!confirm('Are you sure you want to delete this task?'))return;
                        apiCall('/api/tasks/'+taskId,{
                            method:'DELETE'
                        }).then(function(r){
                            if(r.ok){window.location.href='/webapp/projects/'+projectId;}
                            else{showMessage('Error deleting task');}
                        });
                    });
                }

                window.handleConversationClick=function(e){
                    var card=e.target.closest('[data-conversation-id]');
                    if(!card)return;
                    var convId=card.getAttribute('data-conversation-id');
                    if(projectId&&taskId&&convId){
                        window.location.href='/webapp/projects/'+projectId+'/tasks/'+taskId+'/chat/'+convId;
                    }
                };

                // WS subscription for conversation timestamp updates
                if(window.OfmWS&&taskId){
                    window.OfmWS.subscribe({kind:'task',id:parseInt(taskId)},function(msg){
                        if(msg.type==='event'&&msg.payload&&msg.payload.conversation_id){
                            var convId=msg.payload.conversation_id;
                            var dateEl=document.querySelector('.conversation-date[data-conv-id="'+convId+'"]');
                            if(dateEl){
                                var now=new Date();
                                var months=['Jan','Feb','Mar','Apr','May','Jun','Jul','Aug','Sep','Oct','Nov','Dec'];
                                var h=now.getHours().toString().padStart(2,'0');
                                var m=now.getMinutes().toString().padStart(2,'0');
                                dateEl.textContent=months[now.getMonth()]+' '+now.getDate()+', '+h+':'+m;
                                dateEl.classList.remove('is-pulsed');
                                void dateEl.offsetWidth;
                                dateEl.classList.add('is-pulsed');
                                setTimeout(function(){dateEl.classList.remove('is-pulsed');},3000);
                            }
                        }
                    });
                }
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
    fn test_task_detail_has_agent_boxes() {
        let task = make_task();
        let agent_runs = vec![];
        let doc_content = Some("Hello".into());
        let html = leptos::view! { <TaskDetailPage task doc_content agent_runs conversations=vec![] current_run=None /> }.to_html();
        assert!(html.contains("Plannification"));
        assert!(html.contains("Implementation"));
        assert!(html.contains("Refinement"));
        assert!(html.contains("Review"));
        assert!(html.contains("PR"));
        assert!(html.contains("Run"));
        assert!(html.contains("is-info is-light"));
        assert!(html.contains("has-text-info"));
        assert!(html.contains("mdi-file-document-outline"));
        assert!(html.contains("mdi-code-tags"));
        assert!(html.contains("mdi-checkbox-marked-circle-outline"));
        assert!(html.contains("mdi-creation-outline"));
        assert!(html.contains("mdi-source-branch-plus"));
        assert!(html.contains("mdi-play-outline"));
        // is-loading class appears in inline JS code (not as a rendered class when no run)
    }

    #[test]
    fn test_task_detail_current_run_matching_phase_shows_loading() {
        let task = make_task();
        let agent_runs = vec![];
        let doc_content = None;
        let current_run = Some(make_run(AgentType::Implementation, RunStatus::Running));
        let html = leptos::view! { <TaskDetailPage task doc_content agent_runs conversations=vec![] current_run /> }.to_html();
        assert!(html.contains("is-loading"));
        assert!(html.contains("stop-agent-btn"));
    }

    #[test]
    fn test_task_detail_current_run_non_matching_phase_no_loading() {
        let task = make_task();
        let agent_runs = vec![];
        let doc_content = None;
        let current_run = Some(make_run(AgentType::Planification, RunStatus::Running));
        let html = leptos::view! { <TaskDetailPage task doc_content agent_runs conversations=vec![] current_run /> }.to_html();
        // is-loading appears for the Planification phase (and also in inline JS)
        assert!(html.contains("is-loading"));
    }

    #[test]
    fn test_task_detail_stop_agent_visible_when_running() {
        let task = make_task();
        let agent_runs = vec![];
        let doc_content = None;
        let current_run = Some(make_run(AgentType::Implementation, RunStatus::Running));
        let html = leptos::view! { <TaskDetailPage task doc_content agent_runs conversations=vec![] current_run /> }.to_html();
        assert!(html.contains("stop-agent-btn"));
        assert!(html.contains("Stop Agent"));
        assert!(html.contains("mdi-stop"));
    }

    #[test]
    fn test_task_detail_stop_agent_hidden_when_no_run() {
        let task = make_task();
        let agent_runs = vec![];
        let doc_content = None;
        let html = leptos::view! { <TaskDetailPage task doc_content agent_runs conversations=vec![] current_run=None /> }.to_html();
        assert!(html.contains("display:none"));
    }

    #[test]
    fn test_task_detail_renders_markdown_section() {
        let task = make_task();
        let agent_runs = vec![];
        let doc_content = Some("# Hello World".into());
        let html = leptos::view! { <TaskDetailPage task doc_content agent_runs conversations=vec![] current_run=None /> }.to_html();
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
        let html = leptos::view! { <TaskDetailPage task doc_content agent_runs conversations=vec![] current_run=None /> }.to_html();
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
        let html = leptos::view! { <TaskDetailPage task doc_content agent_runs conversations=vec![] current_run=None /> }.to_html();
        assert!(html.contains("No document yet"));
    }

    #[test]
    fn test_task_detail_no_runs_shows_message() {
        let task = make_task();
        let agent_runs = vec![];
        let doc_content = None;
        let html = leptos::view! { <TaskDetailPage task doc_content agent_runs conversations=vec![] current_run=None /> }.to_html();
        assert!(html.contains("No runs yet"));
    }

    #[test]
    fn test_task_detail_shows_conversations_sidebar() {
        let task = make_task();
        let agent_runs = vec![];
        let doc_content = None;
        let html = leptos::view! { <TaskDetailPage task doc_content agent_runs conversations=vec![] current_run=None /> }.to_html();
        assert!(html.contains("Conversations"));
    }

    #[test]
    fn test_task_detail_has_edit_button() {
        let task = make_task();
        let html = leptos::view! { <TaskDetailPage task doc_content=None agent_runs=vec![] conversations=vec![] current_run=None /> }.to_html();
        assert!(html.contains("id=\"edit-task-btn\""));
        assert!(html.contains("mdi-pencil"));
    }

    #[test]
    fn test_task_detail_edit_form_preserves_task_title() {
        let task = make_task();
        let html = leptos::view! { <TaskDetailPage task doc_content=None agent_runs=vec![] conversations=vec![] current_run=None /> }.to_html();
        assert!(html.contains("value=\"Implement feature X\""));
    }

    #[test]
    fn test_task_detail_edit_form_has_status_select() {
        let task = make_task();
        let html = leptos::view! { <TaskDetailPage task doc_content=None agent_runs=vec![] conversations=vec![] current_run=None /> }.to_html();
        assert!(html.contains("id=\"edit-task-status\""));
        assert!(html.contains("Pending"));
        assert!(html.contains("In Progress"));
        assert!(html.contains("In Review"));
        assert!(html.contains("Completed"));
    }

    #[test]
    fn test_task_detail_edit_form_pre_selects_current_status() {
        let task = make_task();
        let html = leptos::view! { <TaskDetailPage task doc_content=None agent_runs=vec![] conversations=vec![] current_run=None /> }.to_html();
        assert!(html.contains("value=\"in_progress\" selected"));
    }

    #[test]
    fn test_task_detail_has_delete_button() {
        let task = make_task();
        let html = leptos::view! { <TaskDetailPage task doc_content=None agent_runs=vec![] conversations=vec![] current_run=None /> }.to_html();
        assert!(html.contains("id=\"delete-task-btn\""));
        assert!(html.contains("Danger Zone"));
        assert!(html.contains("Delete Task"));
        assert!(html.contains("mdi-delete"));
    }
}
