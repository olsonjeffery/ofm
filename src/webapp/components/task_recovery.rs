use crate::db::schema::Task;
use crate::orchestration::MAX_WORKFLOW_RUNS;
use leptos::prelude::*;

/// Blocked/cap banner for a task that cannot advance the workflow: the task hit
/// the per-task iteration cap (`MAX_WORKFLOW_RUNS`) or was blocked by the
/// review agent. Renders only when a recovery action is actually needed, and
/// offers Reset cap & unblock / Recreate with fresh history / Duplicate.
///
/// Carries its own button wiring, reading `taskId`/`projectId` from its own
/// `data-*` attributes so both embedding pages (task_detail.rs and chat.rs)
/// need no page-level glue.
#[component]
pub fn TaskRecoveryBanner(task: Task) -> impl IntoView {
    let capped = task.workflow_run_count >= MAX_WORKFLOW_RUNS;
    if !task.workflow_blocked && !capped {
        return ().into_any();
    }

    let body = if capped {
        format!(
            "This task hit the max agent-run cap ({} agent runs). Agent sessions kept failing, so the iteration budget was burned through without progress. Reset the cap, recreate with fresh history, or duplicate the task.",
            MAX_WORKFLOW_RUNS
        )
    } else {
        "This task was blocked by the review agent. Reset the cap, recreate with fresh history, or duplicate the task."
            .to_string()
    };

    view! {
        <div id="task-recovery-banner" data-task-id={task.id.to_string()} data-project-id={task.project_id.to_string()} class="notification is-danger" style="margin-top:0.5rem">
            <div class="level is-mobile">
                <div class="level-left">
                    <div>
                        <strong class="is-size-5">"Task is blocked"</strong>
                        <p style="margin-top:0.25rem">{body}</p>
                        <span class="tag is-light mt-2">{format!("Agent runs: {}/{}", task.workflow_run_count, MAX_WORKFLOW_RUNS)}</span>
                    </div>
                </div>
                <div class="level-right">
                    <div class="buttons">
                        <button id="reset-cap-btn" class="button is-small is-success has-text-white" title="Keep this conversation and zero the run counter">
                            <span class="icon is-small"><i class="mdi mdi-refresh"></i></span>
                            <span>"Reset cap & unblock"</span>
                        </button>
                        <button id="reset-history-btn" class="button is-small is-warning has-text-white" title="Delete all conversations for this task and start fresh">
                            <span class="icon is-small"><i class="mdi mdi-file-restore-outline"></i></span>
                            <span>"Recreate with fresh history"</span>
                        </button>
                        <button id="duplicate-task-btn" class="button is-small is-info has-text-white" title="Create a copy of this task with a fresh worktree">
                            <span class="icon is-small"><i class="mdi mdi-content-copy"></i></span>
                            <span>"Duplicate task"</span>
                        </button>
                    </div>
                </div>
            </div>
        </div>
        <script>{RECOVERY_JS}</script>
    }
    .into_any()
}

const RECOVERY_JS: &str = r#"(function(){
    var banner=document.getElementById('task-recovery-banner');
    if(!banner)return;
    var taskId=banner.getAttribute('data-task-id');
    var projectId=banner.getAttribute('data-project-id');
    function showMessage(msg){
        var existing=document.getElementById('recovery-message');
        if(existing)existing.remove();
        var div=document.createElement('div');
        div.id='recovery-message';
        div.className='notification is-warning';
        div.style='position:fixed;top:4rem;right:1rem;z-index:9999;';
        div.textContent=msg;
        document.body.appendChild(div);
        setTimeout(function(){div.remove();},5000);
    }
    var resetCapBtn=document.getElementById('reset-cap-btn');
    if(resetCapBtn){
        resetCapBtn.addEventListener('click',function(){
            apiCall('/api/tasks/'+taskId+'/reset-cap',{method:'POST'}).then(function(r){
                if(r.ok){window.location.reload();}
                else{showMessage('Failed to reset cap');}
            });
        });
    }
    var resetHistoryBtn=document.getElementById('reset-history-btn');
    if(resetHistoryBtn){
        resetHistoryBtn.addEventListener('click',function(){
            if(!confirm('This deletes all conversations for this task and resets its workflow state. Continue?'))return;
            apiCall('/api/tasks/'+taskId+'/reset-history',{method:'POST'}).then(function(r){
                if(r.ok){window.location.reload();}
                else{showMessage('Failed to reset history');}
            });
        });
    }
    var duplicateBtn=document.getElementById('duplicate-task-btn');
    if(duplicateBtn){
        duplicateBtn.addEventListener('click',function(){
            apiCall('/api/tasks/'+taskId+'/duplicate',{method:'POST'}).then(function(r){
                if(r.ok){
                    r.json().then(function(body){
                        if(body&&body.id){
                            window.location.href='/webapp/projects/'+projectId+'/tasks/'+body.id;
                        }else{
                            showMessage('Failed to duplicate task');
                        }
                    }).catch(function(){showMessage('Failed to duplicate task');});
                }else{
                    showMessage('Failed to duplicate task');
                }
            });
        });
    }
})();"#;

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDateTime;
    use uuid::Uuid;

    fn make_task(blocked: bool, run_count: i32) -> Task {
        Task {
            id: 1,
            project_id: 1,
            user_id: Uuid::new_v4(),
            title: "Recovery Task".into(),
            status: "in_progress".into(),
            workflow_blocked: blocked,
            workflow_run_count: run_count,
            yolo_mode: false,
            created_at: NaiveDateTime::parse_from_str("2024-06-01 12:00:00", "%Y-%m-%d %H:%M:%S")
                .unwrap(),
        }
    }

    #[test]
    fn test_banner_renders_when_workflow_blocked() {
        let task = make_task(true, 3);
        let html = leptos::view! { <TaskRecoveryBanner task /> }.to_html();
        assert!(html.contains("id=\"task-recovery-banner\""));
        assert!(html.contains("Task is blocked"));
        assert!(html.contains("was blocked by the review agent"));
        assert!(html.contains("Agent runs: 3/25"));
        assert!(html.contains("id=\"reset-cap-btn\""));
        assert!(html.contains("id=\"reset-history-btn\""));
        assert!(html.contains("id=\"duplicate-task-btn\""));
    }

    #[test]
    fn test_banner_renders_when_run_count_at_cap() {
        let task = make_task(false, 25);
        let html = leptos::view! { <TaskRecoveryBanner task /> }.to_html();
        assert!(html.contains("id=\"task-recovery-banner\""));
        assert!(html.contains("hit the max agent-run cap"));
        assert!(html.contains("Agent runs: 25/25"));
    }

    #[test]
    fn test_banner_absent_when_healthy() {
        let task = make_task(false, 0);
        let html = leptos::view! { <TaskRecoveryBanner task /> }.to_html();
        assert!(!html.contains("task-recovery-banner"));
        assert!(!html.contains("reset-cap-btn"));
    }

    #[test]
    fn test_banner_absent_under_cap_without_block() {
        let task = make_task(false, 24);
        let html = leptos::view! { <TaskRecoveryBanner task /> }.to_html();
        assert!(!html.contains("task-recovery-banner"));
    }

    #[test]
    fn test_banner_uses_white_text_buttons_without_is_light() {
        let task = make_task(true, 1);
        let html = leptos::view! { <TaskRecoveryBanner task /> }.to_html();
        assert!(html.contains("has-text-white"));
        assert!(html.contains("is-success"));
        assert!(html.contains("is-warning"));
        assert!(html.contains("is-info"));
        assert!(
            !html.contains("is-light has-text-white"),
            "action buttons must not be is-light"
        );
    }
}
