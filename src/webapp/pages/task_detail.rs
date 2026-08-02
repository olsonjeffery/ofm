use leptos::prelude::*;

use crate::db::schema::{ConversationWithRun, Task};
use crate::webapp::components::conversation_list::ConversationList;
use crate::webapp::components::markdown_viewer::MarkdownViewer;

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
        "in_progress" => "is-info is-light",
        "in_review" => "is-warning is-light",
        "completed" => "is-success is-light",
        _ => "is-light",
    }
}

#[component]
pub fn TaskDetailPage(
    task: Task,
    doc_content: Option<String>,
    conversations: Vec<ConversationWithRun>,
    worktree_missing: bool,
) -> impl IntoView {
    let status_badge_class = status_class(&task.status);
    let status_label_str = status_label(&task.status);
    let task_id = task.id.to_string();
    let conversation_count = conversations.len();

    let doc_value = doc_content.clone().unwrap_or_default();

    view! {
        <section class="section">
            <div class="level" data-task-id={task.id.to_string()} data-project-id={task.project_id.to_string()}>
                <div class="level-left">
                    <div class="level">
                        <div class="level-left">
                            <h1 class="title">{task.title.clone()}</h1>
                        </div>
                        <div class="level-right">
                            <span class={format!("tag {} ml-2", status_badge_class)}>{status_label_str}</span>
                        </div>
                    </div>
                </div>
                <div class="level-right">
                    <button id="edit-task-btn" class="button is-small is-light" title="Edit task">
                        <span class="icon is-small"><i class="mdi mdi-pencil"></i></span>
                        <span>"Edit"</span>
                    </button>
                </div>
            </div>

            {worktree_missing.then(|| {
                view! {
                    <div id="worktree-missing-banner" class="notification is-primary is-light" style="margin-top:0.5rem">
                        <div class="level is-mobile">
                            <div class="level-left">
                                "The worktree directory for this task is missing."
                            </div>
                            <div class="level-right">
                                <button id="recreate-worktree-btn" class="button is-small is-primary has-text-white" title="Recreate worktree">
                                    <span class="icon is-small"><i class="mdi mdi-folder-plus-outline"></i></span>
                                    <span>"Recreate worktree"</span>
                                </button>
                            </div>
                        </div>
                    </div>
                }
                .into_any()
            })}

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
                            <textarea id="edit-task-doc" name="doc_content" class="textarea" rows="10">{doc_value.clone()}</textarea>
                        </div>
                    </div>
                    <div class="field is-grouped is-grouped-right">
                        <div class="control">
                            <button type="submit" class="button is-small is-success">"Save"</button>
                        </div>
                        <div class="control">
                            <button type="button" id="cancel-edit-task-btn" class="button is-small is-light">"Cancel"</button>
                        </div>
                    </div>
                </form>
                <div class="danger-zone">
                    <span class="danger-zone-corner danger-zone-corner-tl"><h2 class="title is-4 has-text-danger">"DANGER ZONE"</h2></span>
                    <span class="danger-zone-corner danger-zone-corner-tr"><h2 class="title is-4 has-text-danger">"DANGER ZONE"</h2></span>
                    <div class="has-text-centered">
                        <button id="delete-task-btn" class="button is-small is-danger">
                            <span class="icon is-small"><i class="mdi mdi-delete"></i></span>
                            <span>"Delete Task"</span>
                        </button>
                    </div>
                    <span class="danger-zone-corner danger-zone-corner-bl"><h2 class="title is-4 has-text-danger">"DANGER ZONE"</h2></span>
                    <span class="danger-zone-corner danger-zone-corner-br"><h2 class="title is-4 has-text-danger">"DANGER ZONE"</h2></span>
                </div>
            </div>

            <div class="columns">
                <div class="column is-one-quarter" style="overflow-y:auto;position:sticky;display:inline-block;scrollbar:hidden">
                    <div class="level is-mobile" style="margin-bottom:0.5rem">
                        <div class="level-left">
                            <h2 class="title is-5">"Conversations "</h2>
                        </div>
                        <div class="level-right">
                        <span class="tag is-grey is-light ml-1">{conversation_count}</span>
                        </div>
                    </div>
                    <ConversationList conversations=conversations active_id=None task_id />
                </div>

                <div class="column" style="overflow-y:auto;height:80vh;">
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
                </div>
            </div>
        </section>
        <script>
            {r#"document.addEventListener('DOMContentLoaded',function(){
                var taskIdEl=document.querySelector('[data-task-id]');
                var taskId=taskIdEl?.getAttribute('data-task-id');
                var projectId=taskIdEl?.getAttribute('data-project-id');

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

                // Recreate worktree button
                var recreateBtn=document.getElementById('recreate-worktree-btn');
                if(recreateBtn){
                    recreateBtn.addEventListener('click',function(){
                        recreateBtn.disabled=true;
                        recreateBtn.classList.add('is-loading');
                        apiCall('/api/tasks/'+taskId+'/worktree/recreate',{
                            method:'POST'
                        }).then(function(r){
                            if(r.ok){window.location.reload();}
                            else{showMessage('Failed to recreate worktree');}
                        }).finally(function(){
                            recreateBtn.disabled=false;
                            recreateBtn.classList.remove('is-loading');
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
                        if(msg.type==='event'){
                            if(msg.event_type==='task_updated'){
                                window.location.reload();
                                return;
                            }
                            if(msg.payload&&msg.payload.conversation_id){
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

    #[test]
    fn test_task_detail_renders_markdown_section() {
        let task = make_task();
        let doc_content = Some("# Hello World".into());
        let html =
            leptos::view! { <TaskDetailPage task doc_content conversations=vec![] worktree_missing=false /> }.to_html();
        assert!(html.contains("Documentation"));
        assert!(html.contains("<h1>"));
        assert!(html.contains("Hello World"));
    }

    #[test]
    fn test_task_detail_empty_doc_shows_prompt() {
        let task = make_task();
        let doc_content = None;
        let html =
            leptos::view! { <TaskDetailPage task doc_content conversations=vec![] worktree_missing=false /> }.to_html();
        assert!(html.contains("No document yet"));
    }

    #[test]
    fn test_task_detail_shows_conversations_sidebar() {
        let task = make_task();
        let doc_content = None;
        let html =
            leptos::view! { <TaskDetailPage task doc_content conversations=vec![] worktree_missing=false /> }.to_html();
        assert!(html.contains("Conversations"));
    }

    #[test]
    fn test_task_detail_has_edit_button() {
        let task = make_task();
        let html = leptos::view! { <TaskDetailPage task doc_content=None conversations=vec![] worktree_missing=false /> }
            .to_html();
        assert!(html.contains("id=\"edit-task-btn\""));
        assert!(html.contains("mdi-pencil"));
    }

    #[test]
    fn test_task_detail_edit_form_preserves_task_title() {
        let task = make_task();
        let html = leptos::view! { <TaskDetailPage task doc_content=None conversations=vec![] worktree_missing=false /> }
            .to_html();
        assert!(html.contains("value=\"Implement feature X\""));
    }

    #[test]
    fn test_task_detail_edit_form_has_status_select() {
        let task = make_task();
        let html = leptos::view! { <TaskDetailPage task doc_content=None conversations=vec![] worktree_missing=false /> }
            .to_html();
        assert!(html.contains("id=\"edit-task-status\""));
        assert!(html.contains("Pending"));
        assert!(html.contains("In Progress"));
        assert!(html.contains("In Review"));
        assert!(html.contains("Completed"));
    }

    #[test]
    fn test_task_detail_edit_form_pre_selects_current_status() {
        let task = make_task();
        let html = leptos::view! { <TaskDetailPage task doc_content=None conversations=vec![] worktree_missing=false /> }
            .to_html();
        assert!(html.contains("value=\"in_progress\" selected"));
    }

    #[test]
    fn test_task_detail_save_cancel_in_right_aligned_group() {
        let task = make_task();
        let html = leptos::view! { <TaskDetailPage task doc_content=None conversations=vec![] worktree_missing=false /> }
            .to_html();
        assert!(
            html.contains(r#"class="field is-grouped is-grouped-right""#),
            "Save / Cancel should be in a right-aligned button group"
        );
        assert!(
            html.contains(r#">Save</button></div>"#),
            "Save should be wrapped in a control"
        );
        assert!(
            html.contains(r#">Cancel</button></div>"#),
            "Cancel should be wrapped in a control"
        );
    }

    #[test]
    fn test_task_detail_danger_zone_in_edit_form() {
        let task = make_task();
        let html = leptos::view! { <TaskDetailPage task doc_content=None conversations=vec![] worktree_missing=false /> }
            .to_html();
        assert!(html.contains("id=\"delete-task-btn\""));
        assert!(html.contains("DANGER ZONE"));
        assert!(html.contains("Delete Task"));
        assert!(html.contains("mdi-delete"));
        assert!(html.contains("edit-task-form"));
        let edit_form_start = html.find("edit-task-form").unwrap();
        let danger_zone_pos = html.find("DANGER ZONE").unwrap();
        assert!(
            danger_zone_pos > edit_form_start,
            "Danger Zone should be inside edit form"
        );
    }

    #[test]
    fn test_task_detail_danger_zone_box_markup() {
        let task = make_task();
        let html = leptos::view! { <TaskDetailPage task doc_content=None conversations=vec![] worktree_missing=false /> }
            .to_html();
        assert!(
            html.contains(r#"class="danger-zone""#),
            "danger zone should use the bordered danger-zone box"
        );
        assert!(
            html.contains(r#"class="has-text-centered""#),
            "delete button should be centered inside the danger zone"
        );
        for corner in [
            "danger-zone-corner-tl",
            "danger-zone-corner-tr",
            "danger-zone-corner-bl",
            "danger-zone-corner-br",
        ] {
            assert_eq!(
                html.matches(corner).count(),
                1,
                "expected one {corner} corner"
            );
        }
        assert_eq!(html.matches("DANGER ZONE").count(), 4);
        let edit_form_start = html.find("edit-task-form").unwrap();
        let edit_form_end = html.rfind("edit-task-form").unwrap() + "edit-task-form".len();
        let edit_form_slice = &html[edit_form_start..edit_form_end];
        assert!(
            !edit_form_slice.contains("<hr"),
            "the horizontal divider should be removed from the edit-task form"
        );
    }

    #[test]
    fn test_task_detail_ws_subscription_reloads_on_task_updated() {
        let task = make_task();
        let html = leptos::view! { <TaskDetailPage task doc_content=None conversations=vec![] worktree_missing=false /> }
            .to_html();
        assert!(html.contains("task_updated"));
        assert!(html.contains("window.location.reload()"));
    }

    #[test]
    fn test_task_detail_status_badge_includes_is_light() {
        let task = make_task();
        let html = leptos::view! { <TaskDetailPage task doc_content=None conversations=vec![] worktree_missing=false /> }
            .to_html();
        assert!(html.contains("is-info is-light"));
    }

    #[test]
    fn test_task_detail_worktree_missing_banner() {
        let task = make_task();
        let html = leptos::view! { <TaskDetailPage task doc_content=None conversations=vec![] worktree_missing=true /> }
            .to_html();
        assert!(html.contains("id=\"worktree-missing-banner\""));
        assert!(html.contains("notification is-primary is-light"));
        assert!(html.contains("The worktree directory for this task is missing."));
        assert!(html.contains("Recreate worktree"));
        assert!(html.contains("mdi-folder-plus-outline"));
        assert!(html.contains("id=\"recreate-worktree-btn\""));
        assert!(html.contains("has-text-white"));
        assert!(html.contains("/api/tasks/'+taskId+'/worktree/recreate"));
        assert!(html.contains("is-loading"));
        let header_pos = html.find("data-task-id").unwrap();
        let banner_pos = html.find("worktree-missing-banner").unwrap();
        assert!(
            banner_pos > header_pos,
            "banner should render under the task title header"
        );
        let form_pos = html.find("edit-task-form").unwrap();
        assert!(
            banner_pos < form_pos,
            "banner should render above the edit form"
        );
    }

    #[test]
    fn test_task_detail_worktree_present_hides_banner() {
        let task = make_task();
        let html = leptos::view! { <TaskDetailPage task doc_content=None conversations=vec![] worktree_missing=false /> }
            .to_html();
        assert!(!html.contains("id=\"recreate-worktree-btn\""));
        assert!(!html.contains("worktree-missing-banner"));
    }
}
