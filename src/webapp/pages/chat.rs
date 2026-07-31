use crate::db::schema::TaskAgentRun;
use crate::providers::types::ProviderEvent;
use crate::webapp::components::chat_input::ChatInput;
use crate::webapp::components::message_stream::MessageStream;
use leptos::prelude::*;

fn build_chat_js(active_id_str: &str, is_running: bool) -> String {
    let processing_init = if is_running { "true" } else { "false" };
    format!(
        r###"
document.addEventListener('DOMContentLoaded', function() {{
    var currentConversationId = "{active_id_str}";
    var taskId = document.getElementById('chat-form')?.getAttribute('data-task-id');
    var isProcessing = false;
    var isAtBottom = true;
    var streamContainer = document.getElementById('message-stream-container');
    var jumpPill = document.getElementById('jump-to-newest-pill');
    var backToTopPill = document.getElementById('back-to-top-pill');
    var agentBar = document.getElementById('agent-thinking-bar');

    function setProcessing(processing) {{
        isProcessing = processing;
        if (agentBar) agentBar.style.display = processing ? 'flex' : 'none';
        var input = document.getElementById('chat-message-input');
        var sendBtn = document.querySelector('#chat-form button');
        if (input) input.disabled = processing;
        if (sendBtn) sendBtn.disabled = processing;
    }}

    if (currentConversationId) {{
        setProcessing({processing_init});
    }}

    function updateJumpPill() {{
        if (!jumpPill || !streamContainer) return;
        var threshold = 50;
        isAtBottom = (streamContainer.scrollHeight - streamContainer.scrollTop - streamContainer.clientHeight) < threshold;
        jumpPill.style.display = isAtBottom ? 'none' : 'block';
    }}

    function updateBackToTopPill() {{
        if (!backToTopPill || !streamContainer) return;
        backToTopPill.style.display = streamContainer.scrollTop > 200 ? 'block' : 'none';
    }}

    // Scroll management
    if (streamContainer) {{
        streamContainer.addEventListener('scroll', function() {{
            updateJumpPill();
            updateBackToTopPill();
        }});
    }}

    function scrollToBottom() {{
        isAtBottom = true;
        if (jumpPill) jumpPill.style.display = 'none';
        if (streamContainer) streamContainer.scrollTop = streamContainer.scrollHeight;
    }}
    window.scrollToBottom = scrollToBottom;

    function scrollToTop() {{
        if (streamContainer) streamContainer.scrollTop = 0;
        if (backToTopPill) backToTopPill.style.display = 'none';
    }}
    window.scrollToTop = scrollToTop;

    scrollToBottom();
    // Periodic check to ensure pill visibility stays correct
    setInterval(function() {{
        updateJumpPill();
        updateBackToTopPill();
    }}, 2000);

    // Format timestamp for display in newest-timestamp pill
    function formatTimestamp(tsStr) {{
        if (!tsStr) return '';
        var now = new Date();
        var parts = tsStr.split(' ');
        if (parts.length < 2) return tsStr;
        var dateParts = parts[0].split('-');
        var timeParts = parts[1].split(':');
        var d = new Date(parseInt(dateParts[0]), parseInt(dateParts[1]) - 1, parseInt(dateParts[2]), parseInt(timeParts[0]), parseInt(timeParts[1]));
        var isToday = d.getDate() === now.getDate() && d.getMonth() === now.getMonth() && d.getFullYear() === now.getFullYear();
        if (isToday) {{
            return ('0' + d.getHours()).slice(-2) + ':' + ('0' + d.getMinutes()).slice(-2);
        }} else {{
            var months = ['Jan','Feb','Mar','Apr','May','Jun','Jul','Aug','Sep','Oct','Nov','Dec'];
            return months[d.getMonth()] + ' ' + ('0' + d.getDate()).slice(-2) + ', ' + ('0' + d.getHours()).slice(-2) + ':' + ('0' + d.getMinutes()).slice(-2);
        }}
    }}

    // Streaming tool-result dedup tracker
    var renderedMessageIds = {{}};

    // Track tool_updated keys separately for outerHTML replacement
    var toolUpdatedKeys = {{}};

    // Stop agent
    window.stopAgent = function() {{
        if (!taskId) return;
        setProcessing(false);
        apiCall('/api/tasks/' + taskId + '/agent-runs/stop', {{
            method: 'POST'
        }}).then(function(r) {{
            if (!r.ok) showMessage('Failed to stop agent');
        }});
    }};

    document.getElementById('chat-form')?.addEventListener('submit', function(e) {{
        e.preventDefault();
        if (!currentConversationId || currentConversationId === '') {{
            showMessage('No conversation selected');
            return;
        }}
        var input = document.getElementById('chat-message-input');
        var text = input ? input.value.trim() : '';
        if (!text) return;
        input.value = '';
        apiCall('/api/tasks/' + taskId + '/conversations/' + currentConversationId + '/messages', {{
            method: 'POST',
            headers: {{ 'Content-Type': 'application/json' }},
            body: JSON.stringify({{ text: text }})
        }}).then(function(r) {{
            if (!r.ok) {{ showMessage('Failed to send message'); }}
        }});
    }});

    // WS event handling with conversation_id filtering
    if (window.OfmWS && taskId) {{
        window.OfmWS.subscribe({{ kind: 'task', id: parseInt(taskId) }}, function(msg) {{
            if (msg.type === 'event') {{
                var convId = msg.payload && msg.payload.conversation_id;
                if (convId && convId !== currentConversationId) return;
                // Any event for this conversation means the agent is active
                setProcessing(true);
                var container = document.getElementById('message-stream');
                if (container) {{
                    // dedup by message_id: merge into existing entry
                    var dedupKey = msg.payload.message_id || msg.payload.tool_use_id || '';
                    if (msg.event_type === 'tool_updated' && dedupKey) {{
                        var el = renderedMessageIds[dedupKey];
                        if (el && msg.html) {{
                            // Replace existing card with server-rendered HTML
                            el.outerHTML = msg.html;
                            var newEl = document.querySelector('[data-tool-use-id="' + dedupKey + '"]');
                            if (newEl) renderedMessageIds[dedupKey] = newEl;
                        }} else if (msg.html) {{
                            // No prior element (Running was suppressed) — append as new content
                            container.insertAdjacentHTML('beforeend', msg.html);
                            renderedMessageIds[dedupKey] = container.lastElementChild;
                        }}
                        if (isAtBottom) {{ scrollToBottom(); }}
                        else {{ updateJumpPill(); }}
                        return;
                    }}
                    if (dedupKey && renderedMessageIds[dedupKey]) {{
                        updateToolCallContent(dedupKey, msg);
                        if (isAtBottom) {{ scrollToBottom(); }}
                        else {{ updateJumpPill(); }}
                        return;
                    }}
                    var eventHtml = renderServerEvent(msg);
                    if (eventHtml) {{
                        // Remove old newest-timestamp pill + its separator before inserting new content
                        var oldNewestPill = document.getElementById('newest-timestamp-pill');
                        if (oldNewestPill) {{
                            oldNewestPill.remove();
                        }}
                        // Prepend user-timestamp pill for user_text events
                        if (msg.event_type === 'user_text' && msg.payload && msg.payload.timestamp) {{
                            var userTsHtml = '<div class="level"><div class="timestamp-pill tag is-light">' + formatTimestamp(msg.payload.timestamp) + '</div></div>';
                            container.insertAdjacentHTML('beforeend', userTsHtml);
                        }}
                        container.insertAdjacentHTML('beforeend', eventHtml);
                        // Append newest-timestamp pill below the new event with separator
                        if (msg.payload && msg.payload.timestamp) {{
                            var newTsHtml = '<div id="newest-timestamp-pill" class="level"><div class="timestamp-pill tag is-light">' + formatTimestamp(msg.payload.timestamp) + '</div></div>';
                            container.insertAdjacentHTML('beforeend', newTsHtml);
                        }}
                        // Track message_id / tool_use_id for future dedup
                        var dk = msg.payload.message_id || msg.payload.tool_use_id || '';
                        if (dk) renderedMessageIds[dk] = container.lastElementChild;
                        if (isAtBottom) {{ scrollToBottom(); }}
                        else {{ updateJumpPill(); }}
                    }}
                }}
            }}
        }});
    }}

    window.toggleCollapse = function(msgId) {{
        var container = document.querySelector('[data-msg-id="' + msgId + '"]');
        if (!container) return;
        container.classList.toggle('is-collapsed');
        container.classList.toggle('is-expanded');
    }};

    function renderServerEvent(msg) {{
        if (!msg.html) return '';
        if (msg.event_type === 'done' || msg.event_type === 'error' || msg.event_type === 'question_asked') {{
            setProcessing(false);
        }}
        return msg.html;
    }}

    function updateToolCallContent(dedupKey, msg) {{
        if (msg.html) {{
            var el = renderedMessageIds[dedupKey];
            if (!el) return;
            el.outerHTML = msg.html;
            var newEl = document.querySelector('[data-tool-use-id="' + dedupKey + '"]');
            if (newEl) renderedMessageIds[dedupKey] = newEl;
        }}
    }}

    function escapeHtml(str) {{
        if (!str) return '';
        var div = document.createElement('div');
        div.appendChild(document.createTextNode(str));
        return div.innerHTML;
    }}

    function showMessage(msg) {{
        var existing = document.getElementById('chat-message-toast');
        if (existing) existing.remove();
        var div = document.createElement('div');
        div.id = 'chat-message-toast';
        div.className = 'notification is-warning';
        div.style = 'position:fixed;top:4rem;right:1rem;z-index:9999;';
        div.textContent = msg;
        document.body.appendChild(div);
        setTimeout(function() {{ div.remove(); }}, 5000);
    }}
}});
"###
    )
}

#[component]
pub fn ChatPage(
    _project_id: i64,
    task_id: i64,
    active_conversation_id: Option<uuid::Uuid>,
    initial_messages: Vec<ProviderEvent>,
    #[allow(unused)] conversation_name: Option<String>,
    current_run: Option<TaskAgentRun>,
) -> impl IntoView {
    let is_running = current_run.as_ref().is_some_and(|r| {
        r.status == crate::db::schema::RunStatus::Running
            && r.conversation_id == active_conversation_id
    });

    let active_id_str = active_conversation_id
        .map(|id| id.to_string())
        .unwrap_or_default();

    let script_content = build_chat_js(&active_id_str, is_running);

    view! {
        <div id="back-to-top-pill"
             style="display:none;position:fixed;top:4rem;left:50%;transform:translateX(-50%);z-index:1000;
                    background:#3273dc;color:#fff;border-radius:2rem;padding:0.25rem 0.75rem;cursor:pointer;
                    box-shadow:0 2px 6px rgba(0,0,0,0.2);font-size:1.1rem;white-space:nowrap;width:auto"
             onclick="window.scrollToTop()">
            <span>"Back to top"</span>
            <span class="icon is-small"><i class="mdi mdi-arrow-up-thick"></i></span>
        </div>
        <div id="chat-layout" style="display:flex;flex-direction:column;height:calc(100vh - 3.75rem);overflow:hidden">
            <div id="message-stream-container" style="flex:1;overflow-y:auto;overflow-x:hidden">
                <MessageStream messages=initial_messages />
            </div>
            <div id="chat-footer" style="border-top:1px solid #ddd;background:#fff;padding:0.5rem 1rem;position:relative">
                <div id="agent-thinking-bar"
                     style="display:none;width:33.33%;margin:0 auto 0.5rem;background:#000;color:#fff;
                             border-radius:8px;padding:0.75rem 1rem;
                             align-items:center;justify-content:space-between;
                             box-shadow:0 2px 8px rgba(0,0,0,0.15)">
                    <span style="display:flex;align-items:center;gap:0.5rem">
                        <span class="icon"><i class="mdi mdi-loading mdi-spin has-text-white"></i></span>
                        <span>"Agent is processing..."</span>
                    </span>
                    <button id="stop-agent-btn" class="button is-primary has-text-white is-small"
                            onclick="stopAgent()">
                        <span class="icon is-small"><i class="mdi mdi-close-thick"></i></span>
                        <span>"Stop Agent"</span>
                    </button>
                </div>
                <div class="chat-input-wrapper" style="position:relative">
                    <div id="jump-to-newest-pill"
                         style="display:none;position:absolute;bottom:65%;left:50%;transform:translateX(-50%);z-index:10;
                                background:#3273dc;color:#fff;border-radius:2rem;padding:0.25rem 0.75rem;cursor:pointer;
                                box-shadow:0 2px 6px rgba(0,0,0,0.2);font-size:1.1rem;white-space:nowrap;width:auto"
                         onclick="window.scrollToBottom()">
                        <span>"Jump to newest"</span>
                        <span class="icon is-small"><i class="mdi mdi-arrow-down-thick"></i></span>
                    </div>
                    <ChatInput
                        _on_send=Callback::new(|_text: String| {
                            // handled by JS interop
                        })
                        disabled=is_running
                        _active_conversation_id=active_conversation_id
                        task_id=task_id
                    />
                </div>
            </div>
        </div>
        <script>
            {script_content}
        </script>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::schema::{AgentType, RunStatus, Task};
    use chrono::NaiveDateTime;

    fn make_task() -> Task {
        Task {
            id: 1,
            project_id: 1,
            user_id: uuid::Uuid::new_v4(),
            title: "Chat Test Task".into(),
            status: "pending".into(),
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
    fn test_chat_page_renders_shell_no_sidebar() {
        let html = leptos::view! {
            <ChatPage
                _project_id=1
                task_id=1
                active_conversation_id=None
                initial_messages=Vec::new()
                conversation_name=None
                current_run=None
            />
        }
        .to_html();
        assert!(
            !html.contains("is-one-quarter"),
            "sidebar should be removed"
        );
        assert!(
            !html.contains("Conversations"),
            "sidebar heading should be removed"
        );
        assert!(html.contains("chat-layout"));
        assert!(html.contains("chat-footer"));
        assert!(html.contains("jump-to-newest-pill"));
        assert!(html.contains("arrow-down-thick"));
        assert!(html.contains("back-to-top-pill"));
        assert!(html.contains("arrow-up-thick"));
        assert!(html.contains("Stop Agent"));
        assert!(html.contains("close-thick"));
        assert!(html.contains("agent-thinking-bar"));
        assert!(html.contains("chat-input-wrapper"));
    }

    #[test]
    fn test_chat_page_with_active_conversation() {
        let conv_id = uuid::Uuid::new_v4();
        let run = TaskAgentRun {
            id: uuid::Uuid::new_v4(),
            task_id: 1,
            agent_type: AgentType::Implementation,
            status: RunStatus::Running,
            conversation_id: Some(conv_id),
            created_at: NaiveDateTime::parse_from_str("2024-06-01 12:00:00", "%Y-%m-%d %H:%M:%S")
                .unwrap(),
            completed_at: None,
        };
        let html = leptos::view! {
            <ChatPage
                _project_id=1
                task_id=1
                active_conversation_id=Some(conv_id)
                initial_messages=Vec::new()
                conversation_name=Some("Test Chat".to_string())
                current_run=Some(run)
            />
        }
        .to_html();
        assert!(html.contains(&conv_id.to_string()));
        assert!(!html.contains("is-one-quarter"));
        assert!(html.contains("chat-layout"));
    }
}
