use crate::db::schema::TaskAgentRun;
use crate::providers::types::ProviderEvent;
use crate::webapp::components::chat_input::ChatInput;
use crate::webapp::components::message_stream::MessageStream;
use leptos::prelude::*;

fn build_chat_js(active_id_str: &str, is_running: bool) -> String {
    let processing_init = if is_running { "true" } else { "false" };
    let js = format!(
        r###"
document.addEventListener('DOMContentLoaded', function() {{
    var currentConversationId = "{active_id_str}";
    var taskId = document.getElementById('chat-form')?.getAttribute('data-task-id');
    var isProcessing = false;
    var isAtBottom = true;
    var streamContainer = document.getElementById('message-stream-container');
    var jumpPill = document.getElementById('jump-to-newest-pill');
    var agentBar = document.getElementById('agent-thinking-bar');

    function setProcessing(processing) {{
        isProcessing = processing;
        if (agentBar) agentBar.style.display = processing ? 'flex' : 'none';
        var input = document.getElementById('chat-message-input');
        var sendBtn = document.querySelector('#chat-form button');
        if (input) input.disabled = processing;
        if (sendBtn) sendBtn.disabled = processing;
    }}

    if (currentConversationId && currentConversationId !== '') {{
        setProcessing({processing_init});
    }}

    function updateJumpPill() {{
        if (!jumpPill || !streamContainer) return;
        var threshold = 50;
        isAtBottom = (streamContainer.scrollHeight - streamContainer.scrollTop - streamContainer.clientHeight) < threshold;
        jumpPill.style.display = isAtBottom ? 'none' : 'block';
    }}

    // Scroll management
    if (streamContainer) {{
        streamContainer.addEventListener('scroll', updateJumpPill);
    }}

    function scrollToBottom() {{
        isAtBottom = true;
        if (jumpPill) jumpPill.style.display = 'none';
        if (streamContainer) streamContainer.scrollTop = streamContainer.scrollHeight;
    }}
    window.scrollToBottom = scrollToBottom;
    scrollToBottom();
    // Periodic check to ensure pill visibility stays correct
    setInterval(updateJumpPill, 2000);

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
                    var eventHtml = renderServerEvent(msg);
                    if (eventHtml) {{
                        container.insertAdjacentHTML('beforeend', eventHtml);
                        if (isAtBottom) {{ scrollToBottom(); }}
                        else {{ updateJumpPill(); }}
                    }}
                }}
            }}
        }});
    }}

    function userMsgHtml(text) {{
        return '<content class="content message-user" style="display:block;background:#1565c0;color:#fff;padding:0.75rem;border-radius:6px;white-space:pre-wrap;max-width:33%;margin-left:auto">' + escapeHtml(text) + '</content>';
    }}

    function renderEvent(evt) {{
        switch (evt.type) {{
            case 'text': return '<div class="box message-text"><div class="content">' + escapeHtml(evt.text) + '</div></div>';
            case 'user_text': return userMsgHtml(evt.text);
            case 'text_chunk': return '<span class="message-chunk">' + escapeHtml(evt.delta) + '</span>';
            case 'tool_use': return renderToolUse({{ tool_name: evt.tool_name, tool_use_id: evt.tool_use_id, input: evt.input }});
            case 'tool_result': return renderToolResult({{ tool_use_id: evt.tool_use_id, result: evt.result }});
            case 'thinking': return '<div class="box message-thinking"><em style="color:#888;">' + escapeHtml(evt.thinking) + '</em></div>';
            case 'thinking_chunk': return '<span style="color:#888;font-style:italic;">' + escapeHtml(evt.delta) + '</span>';
            case 'context_usage': return '<div class="notification is-light is-small">' + escapeHtml(JSON.stringify(evt.usage)) + '</div>';
            case 'error': return '<div class="notification is-danger is-light">' + escapeHtml(evt.error) + '</div>';
            case 'question_asked':
                setProcessing(false);
                if (!evt.questions) return '';
                var html = '';
                evt.questions.forEach(function(q) {{
                    var hdr = q.header || 'Question';
                    var opts = '';
                    if (q.options) {{
                        q.options.forEach(function(o) {{
                            opts += '<span class="tag is-info is-light" style="margin:0.15rem">' + escapeHtml(o.label) + '</span> ';
                        }});
                    }}
                    html += '<div class="box"><strong>' + escapeHtml(hdr) + '</strong><p>' + escapeHtml(q.question) + '</p><div style="margin-top:0.5rem">' + opts + '</div></div>';
                }});
                return html;
            case 'done': return '<div class="notification is-success is-light">Done</div>';
            default: return '';
        }}
    }}

    function renderServerEvent(msg) {{
        switch (msg.event_type) {{
            case 'response':
            case 'ready':
            case 'start':
            case 'extension_ui_request':
            case 'available_commands_update': return '';
            case 'user_text': return userMsgHtml(msg.payload.text || '');
            case 'text': return '<div class="box message-text"><div class="content">' + escapeHtml(msg.payload.text || '') + '</div></div>';
            case 'text_chunk':
                if (msg.payload.delta) {{
                    setProcessing(true);
                    return '<span class="message-chunk">' + escapeHtml(msg.payload.delta) + '</span>';
                }}
                return '';
            case 'tool_use': return renderToolUse(msg.payload);
            case 'tool_result': return renderToolResult(msg.payload);
            case 'thinking': return '<div class="box message-thinking"><em style="color:#888;">' + escapeHtml(msg.payload.thinking || '') + '</em></div>';
            case 'thinking_chunk':
                if (msg.payload.delta) {{
                    setProcessing(true);
                    return '<span style="color:#888;font-style:italic;">' + escapeHtml(msg.payload.delta) + '</span>';
                }}
                return '';
            case 'context_usage': return '<div class="notification is-light is-small">' + escapeHtml(JSON.stringify(msg.payload.usage || {{}})) + '</div>';
            case 'error':
                setProcessing(false);
                return '<div class="notification is-danger is-light">' + escapeHtml(msg.payload.error || '') + '</div>';
            case 'question_asked':
                setProcessing(false);
                var questions = msg.payload.questions || [];
                var qHtml = '';
                questions.forEach(function(q) {{
                    var hdr = q.header || 'Question';
                    var opts = '';
                    if (q.options) {{
                        q.options.forEach(function(o) {{
                            opts += '<span class="tag is-info is-light" style="margin:0.15rem">' + escapeHtml(o.label) + '</span> ';
                        }});
                    }}
                    qHtml += '<div class="box"><strong>' + escapeHtml(hdr) + '</strong><p>' + escapeHtml(q.question) + '</p><div style="margin-top:0.5rem">' + opts + '</div></div>';
                }});
                return qHtml;
            case 'done':
                setProcessing(false);
                return '<div class="notification is-success is-light">Done</div>';
            default: return '';
        }}
    }}

    function renderToolUse(payload) {{
        var toolName = escapeHtml(payload.tool_name || 'unknown');
        var toolId = escapeHtml(payload.tool_use_id || '');
        var inputStr = escapeHtml(JSON.stringify(payload.input, null, 2));
        return '<div class="card"><div class="card-content"><span class="tag is-info is-light">' + toolName + '</span><code>' + toolId + '</code><div id="tool-input-' + toolId + '" class="tool-input-box" style="margin-top:0.25rem"><pre style="white-space:pre-wrap;word-break:break-word;overflow-wrap:break-word;max-width:100%">' + inputStr + '</pre></div></div></div>';
    }}

    function renderToolResult(payload) {{
        var toolId = escapeHtml(payload.tool_use_id || '');
        var result = payload.result || '';
        var truncated = result.length > 100;
        var displayText = truncated ? escapeHtml(result.substring(0, 100)) + '...' : escapeHtml(result);
        var extra = truncated ? '<a href="#" class="toggle-result" data-tool-id="' + toolId + '" onclick="toggleResult(this);return false">show more</a>' : '';
        var fullContent = truncated ? '<div class="tool-result-full" id="result-full-' + toolId + '" style="display:none"><pre style="white-space:pre-wrap;word-break:break-word;overflow-wrap:break-word;max-width:100%">' + escapeHtml(result) + '</pre></div>' : '';
        return '<div class="card"><div class="card-content"><span class="tag is-success is-light">result</span><code>' + toolId + '</code><pre class="tool-result-preview" id="result-preview-' + toolId + '" style="white-space:pre-wrap;word-break:break-word;overflow-wrap:break-word;max-width:100%">' + displayText + '</pre>' + extra + fullContent + '</div></div>';
    }}

    window.toggleResult = function(el) {{
        var toolId = el.getAttribute('data-tool-id');
        var preview = document.getElementById('result-preview-' + toolId);
        var full = document.getElementById('result-full-' + toolId);
        if (preview && full) {{
            var isHidden = full.style.display === 'none';
            full.style.display = isHidden ? 'block' : 'none';
            preview.style.display = isHidden ? 'none' : 'block';
            el.textContent = isHidden ? 'show less' : 'show more';
        }}
    }};

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
    );
    js
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
                        "Jump to newest"
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
