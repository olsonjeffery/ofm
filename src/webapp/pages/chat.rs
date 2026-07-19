use crate::db::schema::{ConversationWithRun, Task, TaskAgentRun};
use crate::webapp::components::chat_input::ChatInput;
use crate::webapp::components::conversation_list::ConversationList;
use crate::webapp::components::message_stream::MessageStream;
use leptos::prelude::*;

#[component]
pub fn ChatPage(
    _project_id: i64,
    task_id: i64,
    _task: Task,
    conversations: Vec<ConversationWithRun>,
    current_run: Option<TaskAgentRun>,
) -> impl IntoView {
    let is_running = current_run
        .as_ref()
        .map(|r| r.status == crate::db::schema::RunStatus::Running)
        .unwrap_or(false);

    let messages = Vec::new();

    view! {
        <div id="chat-layout" style="display:flex;flex-direction:column;height:calc(100vh - 3.75rem);overflow:hidden">
            <div class="columns" style="flex:1;overflow:hidden;display:flex;margin-top:0.5rem">
                <div class="column is-one-quarter" style="border-right:1px solid #ddd;overflow-y:auto">
                    <h2 class="title is-6">"Conversations"</h2>
                    <ConversationList conversations=conversations active_id=None />
                </div>
                <div class="column" style="display:flex;flex-direction:column;overflow:hidden">
                    <div id="message-stream-container" style="flex:1;overflow-y:auto;overflow-x:hidden">
                        <MessageStream messages=messages />
                        <div id="jump-to-newest-pill"
                             style="display:none;position:sticky;bottom:0.5rem;left:50%;transform:translateX(-50%);z-index:10;background:#3273dc;color:#fff;border-radius:2rem;padding:0.5rem 1rem;cursor:pointer;box-shadow:0 2px 6px rgba(0,0,0,0.2);font-size:0.875rem;white-space:nowrap"
                             onclick="window.scrollToBottom()">
                            "↓ Jump to newest"
                        </div>
                    </div>
                </div>
            </div>
            <div id="chat-footer" style="border-top:1px solid #ddd;background:#fff;padding:0.5rem 1rem">
                <div id="agent-thinking-bar" style="display:none;padding:0.5rem 1rem;background:#f5f5f5;border-top:1px solid #ddd">
                    <span class="icon has-text-info" style="margin-right:0.5rem"><i class="mdi mdi-loading mdi-spin"></i></span>
                    <span class="has-text-info">"Agent is processing..."</span>
                </div>
                <div style="display:flex;align-items:center;gap:0.5rem;padding:0.25rem 0">
                    <button id="stop-agent-btn" class="button is-small is-danger is-light" onclick="stopAgent()">"Stop Agent"</button>
                    <ChatInput
                        _on_send=Callback::new(|_text: String| {
                            // handled by JS interop
                        })
                        disabled=is_running
                        _active_conversation_id=None
                        task_id=task_id
                    />
                </div>
            </div>
        </div>
        <script>
            {r##"
document.addEventListener('DOMContentLoaded', function() {
    var currentConversationId = null;
    var taskId = document.getElementById('chat-form')?.getAttribute('data-task-id');
    var isProcessing = false;
    var isAtBottom = true;
    var userScrolledUp = false;
    var streamContainer = document.getElementById('message-stream-container');
    var jumpPill = document.getElementById('jump-to-newest-pill');

    function setProcessing(processing) {
        isProcessing = processing;
        var bar = document.getElementById('agent-thinking-bar');
        var input = document.getElementById('chat-message-input');
        var sendBtn = document.querySelector('#chat-form button');
        if (bar) bar.style.display = processing ? 'flex' : 'none';
        if (input) input.disabled = processing;
        if (sendBtn) sendBtn.disabled = processing;
    }

    // Scroll management
    if (streamContainer) {
        streamContainer.addEventListener('scroll', function() {
            var threshold = 50;
            isAtBottom = (streamContainer.scrollHeight - streamContainer.scrollTop - streamContainer.clientHeight) < threshold;
            if (jumpPill) jumpPill.style.display = isAtBottom ? 'none' : 'block';
        });
    }

    function scrollToBottom() {
        isAtBottom = true;
        if (jumpPill) jumpPill.style.display = 'none';
        if (streamContainer) streamContainer.scrollTop = streamContainer.scrollHeight;
    }
    window.scrollToBottom = scrollToBottom;

    // Stop agent — sweeps all sessions for the task
    window.stopAgent = function() {
        if (!taskId) return;
        setProcessing(false);
        apiCall('/api/tasks/' + taskId + '/agent-runs/stop', {
            method: 'POST'
        }).then(function(r) {
            if (!r.ok) showMessage('Failed to stop agent');
        });
    };



    window.handleConversationClick = function(e) {
        var card = e.target.closest('[data-conversation-id]');
        if (!card) return;
        var convId = card.getAttribute('data-conversation-id');
        currentConversationId = convId;
        document.querySelectorAll('.conversation-list .card').forEach(function(c) { c.classList.remove('is-active'); });
        card.classList.add('is-active');
        fetch('/api/tasks/' + taskId + '/conversations/' + convId)
            .then(function(r) { return r.json(); })
            .then(function(data) {
                var container = document.getElementById('message-stream');
                if (container) {
                    var html = '';
                    if (data.messages && data.messages.length > 0) {
                        data.messages.forEach(function(evt) {
                            html += renderEvent(evt);
                        });
                    } else {
                        html = '<p class="has-text-grey">No messages yet.</p>';
                    }
                    container.innerHTML = html;
                    scrollToBottom();
                }
            });
    };

    document.getElementById('chat-form')?.addEventListener('submit', function(e) {
        e.preventDefault();
        if (!currentConversationId) { showMessage('Select a conversation first'); return; }
        var input = document.getElementById('chat-message-input');
        var text = input ? input.value.trim() : '';
        if (!text) return;
        input.value = '';
        apiCall('/api/tasks/' + taskId + '/conversations/' + currentConversationId + '/messages', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ text: text })
        }).then(function(r) {
            if (!r.ok) { showMessage('Failed to send message'); }
        });
    });

    // WS event handling
    if (window.OfmWS && taskId) {
        window.OfmWS.subscribe({ kind: 'task', id: parseInt(taskId) }, function(msg) {
            if (msg.type === 'event') {
                var container = document.getElementById('message-stream');
                if (container) {
                    var eventHtml = renderServerEvent(msg);
                    if (eventHtml) {
                        container.insertAdjacentHTML('beforeend', eventHtml);
                        if (isAtBottom) { scrollToBottom(); }
                    }
                }
            }
        });
    }

    function userMsgHtml(text) {
        return '<content class="content message-user" style="display:block;background:#1565c0;color:#fff;padding:0.75rem;border-radius:6px;white-space:pre-wrap;max-width:33%;margin-left:auto">' + escapeHtml(text) + '</content>';
    }

    function renderEvent(evt) {
        switch (evt.type) {
            case 'text': return '<div class="box message-text"><div class="content">' + escapeHtml(evt.text) + '</div></div>';
            case 'user_text': return userMsgHtml(evt.text);
            case 'text_chunk': return '<span class="message-chunk">' + escapeHtml(evt.delta) + '</span>';
            case 'tool_use': return renderToolUse({ tool_name: evt.tool_name, tool_use_id: evt.tool_use_id, input: evt.input });
            case 'tool_result': return renderToolResult({ tool_use_id: evt.tool_use_id, result: evt.result });
            case 'thinking': return '<div class="box message-thinking"><em style="color:#888;">' + escapeHtml(evt.thinking) + '</em></div>';
            case 'thinking_chunk': return '<span style="color:#888;font-style:italic;">' + escapeHtml(evt.delta) + '</span>';
            case 'context_usage': return '<div class="notification is-light is-small">' + escapeHtml(JSON.stringify(evt.usage)) + '</div>';
            case 'error': return '<div class="notification is-danger is-light">' + escapeHtml(evt.error) + '</div>';
            case 'question_asked':
                setProcessing(false);
                if (!evt.questions) return '';
                var html = '';
                evt.questions.forEach(function(q) {
                    var hdr = q.header || 'Question';
                    var opts = '';
                    if (q.options) {
                        q.options.forEach(function(o) {
                            opts += '<span class="tag is-info is-light" style="margin:0.15rem">' + escapeHtml(o.label) + '</span> ';
                        });
                    }
                    html += '<div class="box"><strong>' + escapeHtml(hdr) + '</strong><p>' + escapeHtml(q.question) + '</p><div style="margin-top:0.5rem">' + opts + '</div></div>';
                });
                return html;
            case 'done': return '<div class="notification is-success is-light">Done</div>';
            default: return '';
        }
    }

    function renderServerEvent(msg) {
        switch (msg.event_type) {
            case 'response':
            case 'ready':
            case 'start':
            case 'extension_ui_request':
            case 'available_commands_update': return '';
            case 'user_text': return userMsgHtml(msg.payload.text || '');
            case 'text': return '<div class="box message-text"><div class="content">' + escapeHtml(msg.payload.text || '') + '</div></div>';
            case 'text_chunk':
                if (msg.payload.delta) {
                    setProcessing(true);
                    return '<span class="message-chunk">' + escapeHtml(msg.payload.delta) + '</span>';
                }
                return '';
            case 'tool_use': return renderToolUse(msg.payload);
            case 'tool_result': return renderToolResult(msg.payload);
            case 'thinking': return '<div class="box message-thinking"><em style="color:#888;">' + escapeHtml(msg.payload.thinking || '') + '</em></div>';
            case 'thinking_chunk':
                if (msg.payload.delta) {
                    setProcessing(true);
                    return '<span style="color:#888;font-style:italic;">' + escapeHtml(msg.payload.delta) + '</span>';
                }
                return '';
            case 'context_usage': return '<div class="notification is-light is-small">' + escapeHtml(JSON.stringify(msg.payload.usage || {})) + '</div>';
            case 'error': return '<div class="notification is-danger is-light">' + escapeHtml(msg.payload.error || '') + '</div>';
            case 'question_asked':
                setProcessing(false);
                var questions = msg.payload.questions || [];
                var qHtml = '';
                questions.forEach(function(q) {
                    var hdr = q.header || 'Question';
                    var opts = '';
                    if (q.options) {
                        q.options.forEach(function(o) {
                            opts += '<span class="tag is-info is-light" style="margin:0.15rem">' + escapeHtml(o.label) + '</span> ';
                        });
                    }
                    qHtml += '<div class="box"><strong>' + escapeHtml(hdr) + '</strong><p>' + escapeHtml(q.question) + '</p><div style="margin-top:0.5rem">' + opts + '</div></div>';
                });
                return qHtml;
            case 'done':
                setProcessing(false);
                return '<div class="notification is-success is-light">Done</div>';
            default: return '';
        }
    }

    function renderToolUse(payload) {
        var toolName = escapeHtml(payload.tool_name || 'unknown');
        var toolId = escapeHtml(payload.tool_use_id || '');
        var inputStr = escapeHtml(JSON.stringify(payload.input, null, 2));
        return '<div class="card"><div class="card-content"><span class="tag is-info is-light">' + toolName + '</span><code>' + toolId + '</code><div id="tool-input-' + toolId + '" class="tool-input-box" style="margin-top:0.25rem"><pre style="white-space:pre-wrap;word-break:break-word;overflow-wrap:break-word;max-width:100%">' + inputStr + '</pre></div></div></div>';
    }

    function renderToolResult(payload) {
        var toolId = escapeHtml(payload.tool_use_id || '');
        var result = payload.result || '';
        var truncated = result.length > 100;
        var displayText = truncated ? escapeHtml(result.substring(0, 100)) + '...' : escapeHtml(result);
        var extra = truncated ? '<a href="#" class="toggle-result" data-tool-id="' + toolId + '" onclick="toggleResult(this);return false">show more</a>' : '';
        var fullContent = truncated ? '<div class="tool-result-full" id="result-full-' + toolId + '" style="display:none"><pre style="white-space:pre-wrap;word-break:break-word;overflow-wrap:break-word;max-width:100%">' + escapeHtml(result) + '</pre></div>' : '';
        return '<div class="card"><div class="card-content"><span class="tag is-success is-light">result</span><code>' + toolId + '</code><pre class="tool-result-preview" id="result-preview-' + toolId + '" style="white-space:pre-wrap;word-break:break-word;overflow-wrap:break-word;max-width:100%">' + displayText + '</pre>' + extra + fullContent + '</div></div>';
    }

    window.toggleResult = function(el) {
        var toolId = el.getAttribute('data-tool-id');
        var preview = document.getElementById('result-preview-' + toolId);
        var full = document.getElementById('result-full-' + toolId);
        if (preview && full) {
            var isHidden = full.style.display === 'none';
            full.style.display = isHidden ? 'block' : 'none';
            preview.style.display = isHidden ? 'none' : 'block';
            el.textContent = isHidden ? 'show less' : 'show more';
        }
    };

    function escapeHtml(str) {
        if (!str) return '';
        var div = document.createElement('div');
        div.appendChild(document.createTextNode(str));
        return div.innerHTML;
    }

    function showMessage(msg) {
        var existing = document.getElementById('chat-message-toast');
        if (existing) existing.remove();
        var div = document.createElement('div');
        div.id = 'chat-message-toast';
        div.className = 'notification is-warning';
        div.style = 'position:fixed;top:4rem;right:1rem;z-index:9999;';
        div.textContent = msg;
        document.body.appendChild(div);
        setTimeout(function() { div.remove(); }, 5000);
    }
});
            "##}
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
    fn test_chat_page_renders_shell() {
        let task = make_task();
        let html = leptos::view! {
            <ChatPage
                _project_id=1
                task_id=1
                _task=task
                conversations=Vec::new()
                current_run=None
            />
        }
        .to_html();
        assert!(html.contains("Conversations"));
        assert!(html.contains("chat-layout"));
        assert!(html.contains("chat-footer"));
        assert!(html.contains("jump-to-newest-pill"));
        assert!(html.contains("Stop Agent"));
    }
}
