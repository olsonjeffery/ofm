use crate::db::schema::{ConversationWithRun, Task, TaskAgentRun};
use crate::providers::registry::AgentConfigStatus;
use crate::webapp::components::agent_run_banner::AgentRunBanner;
use crate::webapp::components::chat_input::ChatInput;
use crate::webapp::components::conversation_list::ConversationList;
use crate::webapp::components::message_stream::MessageStream;
use leptos::prelude::*;

#[component]
pub fn ChatPage(
    project_id: i64,
    task_id: i64,
    task: Task,
    conversations: Vec<ConversationWithRun>,
    agent_config_statuses: Vec<AgentConfigStatus>,
    current_run: Option<TaskAgentRun>,
) -> impl IntoView {
    let _project_id_str = project_id.to_string();
    let _task_id_str = task_id.to_string();

    let agent_types: Vec<String> = agent_config_statuses
        .iter()
        .filter(|s| s.configured)
        .map(|s| s.agent_type.clone())
        .collect();

    let is_running = current_run
        .as_ref()
        .map(|r| r.status == crate::db::schema::RunStatus::Running)
        .unwrap_or(false);

    let messages = Vec::new();

    view! {
        <section class="section" style="padding-top:1rem;padding-bottom:0">
            <nav class="breadcrumb" aria-label="breadcrumbs">
                <ul>
                    <li><a href="/webapp">"Dashboard"</a></li>
                    <li><a href={format!("/webapp/projects/{}", project_id)}>"Board"</a></li>
                    <li><a href={format!("/webapp/projects/{}/tasks/{}", project_id, task_id)}>{task.title.clone()}</a></li>
                    <li class="is-active"><a href="#">"Chat"</a></li>
                </ul>
            </nav>

            <div class="level">
                <div class="level-left">
                    <h1 class="title is-4">{task.title.clone()}</h1>
                </div>
            </div>

            <AgentRunBanner task=task agent_config_statuses=agent_config_statuses.clone() current_run=current_run />

            <div class="columns" style="margin-top:0.5rem;min-height:60vh">
                <div class="column is-one-quarter" style="border-right:1px solid #ddd">
                    <h2 class="title is-6">"Conversations"</h2>
                    <ConversationList conversations=conversations active_id=None />
                </div>
                <div class="column" style="display:flex;flex-direction:column">
                    <div id="message-stream-container" style="flex:1;overflow-y:auto;min-height:300px">
                        <MessageStream messages=messages />
                    </div>
                    <ChatInput
                        _on_send=Callback::new(|_text: String| {
                            // handled by JS interop
                        })
                        agent_types=agent_types
                        disabled=is_running
                        _active_conversation_id=None
                        task_id=task_id
                    />
                    <div id="agent-thinking-bar" class={if is_running { "is-active" } else { "" }} style="display:none;padding:0.5rem 1rem;background:#f5f5f5;border-top:1px solid #ddd">
                        <span class="icon has-text-info" style="margin-right:0.5rem"><i class="mdi mdi-loading mdi-spin"></i></span>
                        <span class="has-text-info">Agent is processing...</span>
                        <button id="stop-agent-btn" class="button is-small is-danger is-light" style="margin-left:auto" onclick="abortCurrentTurn()">"Stop"</button>
                    </div>
                    <div style="padding:0.25rem 1rem;text-align:right">
                        <button id="reset-agent-btn" class="button is-small is-warning is-light" onclick="resetAgentSession()">"Reset Session"</button>
                    </div>
                </div>
            </div>
        </section>
        <script>
            {r##"
document.addEventListener('DOMContentLoaded', function() {
    var currentConversationId = null;
    var taskId = document.getElementById('chat-form')?.getAttribute('data-task-id');
    var isProcessing = false;

    function setProcessing(processing) {
        isProcessing = processing;
        var bar = document.getElementById('agent-thinking-bar');
        var input = document.getElementById('chat-message-input');
        var sendBtn = document.querySelector('#chat-form button');
        if (bar) bar.style.display = processing ? 'flex' : 'none';
        if (input) input.disabled = processing;
        if (sendBtn) sendBtn.disabled = processing;
    }

    // Stop button
    window.abortCurrentTurn = function() {
        if (!currentConversationId || !taskId) return;
        setProcessing(false);
        apiCall('/api/tasks/' + taskId + '/conversations/' + currentConversationId + '/abort', {
            method: 'POST'
        }).then(function(r) {
            if (!r.ok) showMessage('Failed to abort');
        });
    };

    window.resetAgentSession = function() {
        if (!taskId) return;
        setProcessing(false);
        apiCall('/api/tasks/' + taskId + '/agent-runs/reset', {
            method: 'POST'
        }).then(function(r) {
            if (r.ok) {
                showMessage('Session reset. You can now start a new agent run.');
                setTimeout(function() { window.location.reload(); }, 2000);
            } else {
                showMessage('Failed to reset session');
            }
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
                    var streamContainer = document.getElementById('message-stream-container');
                    if (streamContainer) streamContainer.scrollTop = streamContainer.scrollHeight;
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

    document.getElementById('start-agent-run-btn')?.addEventListener('click', function() {
        var btn = this;
        btn.disabled = true;
        btn.classList.add('is-loading');
        apiCall('/api/tasks/' + taskId + '/agent-runs', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ agent_type: 'implementation' })
        }).then(function(r) {
            if (r.ok) { window.location.reload(); }
            else { showMessage('Failed to start agent run'); }
        }).finally(function() {
            btn.disabled = false;
            btn.classList.remove('is-loading');
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
                        var streamContainer = document.getElementById('message-stream-container');
                        if (streamContainer) streamContainer.scrollTop = streamContainer.scrollHeight;
                    }
                }
            }
        });
    }

    function userMsgHtml(text) {
        return '<div class="box message-user" style="background:#1565c0;color:#fff;border-left:4px solid #0d47a1"><content style="white-space:pre-wrap">' + escapeHtml(text) + '</content></div>';
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
        return '<div class="card"><div class="card-content"><span class="tag is-info is-light">' + toolName + '</span><code>' + toolId + '</code><div id="tool-input-' + toolId + '" class="tool-input-box" style="margin-top:0.25rem"><pre>' + inputStr + '</pre></div></div></div>';
    }

    function renderToolResult(payload) {
        var toolId = escapeHtml(payload.tool_use_id || '');
        var result = payload.result || '';
        var truncated = result.length > 100;
        var displayText = truncated ? escapeHtml(result.substring(0, 100)) + '...' : escapeHtml(result);
        var extra = truncated ? '<a href="#" class="toggle-result" data-tool-id="' + toolId + '" onclick="toggleResult(this);return false">show more</a>' : '';
        var fullContent = truncated ? '<div class="tool-result-full" id="result-full-' + toolId + '" style="display:none"><pre>' + escapeHtml(result) + '</pre></div>' : '';
        return '<div class="card"><div class="card-content"><span class="tag is-success is-light">result</span><code>' + toolId + '</code><pre class="tool-result-preview" id="result-preview-' + toolId + '">' + displayText + '</pre>' + extra + fullContent + '</div></div>';
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
                project_id=1
                task_id=1
                task
                conversations=Vec::new()
                agent_config_statuses=Vec::new()
                current_run=None
            />
        }
        .to_html();
        assert!(html.contains("Chat Test Task"));
        assert!(html.contains("Chat"));
        assert!(html.contains("Conversations"));
        assert!(html.contains("Start Agent Run"));
    }
}
