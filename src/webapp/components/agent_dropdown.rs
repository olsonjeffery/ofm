use crate::db::schema::ActiveAgent;
use crate::webapp::components::breadcrumb::title_truncate;
use leptos::prelude::*;

#[component]
pub fn AgentDropdown(active_agents: Vec<ActiveAgent>) -> impl IntoView {
    let count = active_agents.len();

    let agent_items: Vec<_> = active_agents
        .iter()
        .map(|agent| {
            let conv_name = agent.conversation_name.as_deref().unwrap_or("Unnamed");
            let href = format!(
                "/webapp/projects/{}/tasks/{}/chat/{}",
                agent.project_id, agent.task_id, agent.conversation_id
            );
            let icon_class = format!("mdi mdi-{}", agent.agent_type.icon());
            let label = format!(
                "{}/{}: {}",
                title_truncate(&agent.project_title),
                title_truncate(&agent.task_title),
                title_truncate(conv_name),
            );
            view! {
                <a class="dropdown-item" href=href>
                    <span class="icon is-small"><i class=icon_class></i></span>
                    <span>{label}</span>
                </a>
            }
        })
        .collect();

    view! {
        <div class="navbar-item">
            <div class="dropdown" id="agent-dropdown">
                <div class="dropdown-trigger">
                    <div class="buttons has-addons">
                        <button
                                class="button is-small"
                                aria-haspopup="true"
                                aria-controls="agent-dropdown-menu"
                                id="agent-dropdown-trigger"
                            >
                                <span class="icon"><i class="mdi mdi-message-outline" id="agent-message-icon"></i></span>
                                <span id="agent-count">{format!("{} Agents", count)}</span>
                        </button>
                        <button
                                class="button is-small"
                                aria-haspopup="true"
                                aria-controls="agent-dropdown-menu"
                                id="agent-dropdown-trigger-2"
                            >
                                <span class="icon"><i class="mdi mdi-arrow-down-bold"></i></span>
                        </button>
                    </div>
                </div>
                <div class="dropdown-menu" id="agent-dropdown-menu" role="menu">
                    <div class="dropdown-content">
                        <div class="dropdown-item" id="ws-status-entry">
                            <span class="icon is-small"><i class="mdi mdi-wifi" id="ws-icon"></i></span>
                            <span id="ws-label">"Connected"</span>
                            <span id="ws-last-payload" class="has-text-grey-dark is-size-7" style="margin-left: auto">"No payloads yet"</span>
                        </div>
                        <div id="agent-entries">
                            {if !active_agents.is_empty() {
                                view! {
                                    <hr class="dropdown-divider"/>
                                    {agent_items}
                                }.into_any()
                            } else {
                                ().into_any()
                            }}
                        </div>
                        <div id="agent-question-entries"></div>
                        <div id="agent-blocked-entries"></div>
                    </div>
                </div>
            </div>
        </div>
        <script>
            {r#"(function(){
                var dd = document.getElementById('agent-dropdown');
                var trigger = document.getElementById('agent-dropdown-trigger');
                if (trigger) {
                    trigger.addEventListener('click', function(ev) {
                        ev.stopPropagation();
                        dd.classList.toggle('is-active');
                    });
                }
                trigger = document.getElementById('agent-dropdown-trigger-2');
                if (trigger) {
                    trigger.addEventListener('click', function(ev) {
                        ev.stopPropagation();
                        dd.classList.toggle('is-active');
                    });
                }
                document.addEventListener('click', function(ev) {
                    if (dd && !dd.contains(ev.target)) {
                        dd.classList.remove('is-active');
                    }
                });

                var el = document.getElementById('ws-status-entry');
                var icon = document.getElementById('ws-icon');
                var label = document.getElementById('ws-label');
                var payload = document.getElementById('ws-last-payload');

                function fmtAgo(ts) {
                    var s = Math.floor((Date.now() - ts) / 1000);
                    if (s < 5) return 'Just now';
                    if (s < 60) return s + 's ago';
                    var m = Math.floor(s / 60);
                    if (m < 60) return m + 'm ago';
                    var h = Math.floor(m / 60);
                    return h + 'h ago';
                }

                function updateConn(status) {
                    if (status === 'connected') {
                        icon.className = 'mdi mdi-wifi';
                        el.classList.remove('is-primary');
                        el.classList.add('is-success');
                        label.textContent = 'Connected';
                    } else if (status === 'connecting') {
                        icon.className = 'mdi mdi-wifi-off';
                        el.classList.remove('is-primary', 'is-success');
                        label.textContent = 'Connecting...';
                    } else {
                        icon.className = 'mdi mdi-wifi-off';
                        el.classList.remove('is-success');
                        el.classList.add('is-primary');
                        label.textContent = 'Disconnected';
                    }
                }

                function updatePayload(ts) {
                    payload.textContent = 'Last: ' + fmtAgo(ts);
                }

                if (window.OfmWS) {
                    updateConn(window.OfmWS.status);
                    if (window.OfmWS._lastPayloadTime) {
                        updatePayload(window.OfmWS._lastPayloadTime);
                    }
                }

                document.addEventListener('ws-status-changed', function(ev) {
                    updateConn(ev.detail.status);
                });

                document.addEventListener('ws-payload-received', function(ev) {
                    updatePayload(ev.detail.timestamp);
                });

                setInterval(function() {
                    if (window.OfmWS && window.OfmWS._lastPayloadTime) {
                        updatePayload(window.OfmWS._lastPayloadTime);
                    }
                }, 10000);

                // Global agent status — driven exclusively by server activity.
                // The System-topic `agent_status` broadcasts are the source of
                // truth; a lightweight periodic re-sync covers any missed frame.
                var AGENT_ICONS = {
                    'planification': 'file-document-outline',
                    'implementation': 'code-tags',
                    'refinement': 'creation-outline',
                    'review': 'checkbox-marked-circle-outline',
                    'pr': 'source-branch-plus',
                    'yolo': 'rocket'
                };

                function truncate(s, maxLen) {
                    if (s.length <= maxLen) return s;
                    return s.substring(0, maxLen) + '\u2026';
                }

                function escapeHtml(s) {
                    var d = document.createElement('div');
                    d.appendChild(document.createTextNode(s));
                    return d.innerHTML;
                }

                function entryLink(projectId, taskId) {
                    return '/webapp/projects/' + projectId + '/tasks/' + taskId;
                }

                function taskEntryHtml(summary, icon) {
                    var label = truncate(summary.project_title, 24) + '/' + truncate(summary.task_title, 24);
                    return '<a class="dropdown-item" href="' + entryLink(summary.project_id, summary.task_id) + '">' +
                        '<span class="icon is-small"><i class="mdi mdi-' + icon + '"></i></span>' +
                        '<span>' + escapeHtml(label) + '</span>' +
                        '</a>';
                }

                function renderAgentEntries(agents) {
                    var container = document.getElementById('agent-entries');
                    if (!container) return;
                    var html = '';
                    if (agents.length > 0) {
                        html += '<hr class="dropdown-divider">';
                        for (var i = 0; i < agents.length; i++) {
                            var a = agents[i];
                            var icon = AGENT_ICONS[a.agent_type] || 'help-circle';
                            var convName = a.conversation_name || 'Unnamed';
                            var label = truncate(a.project_title, 24) + '/' + truncate(a.task_title, 24) + ': ' + truncate(convName, 24);
                            html += '<a class="dropdown-item" href="/webapp/projects/' + a.project_id + '/tasks/' + a.task_id + '/chat/' + a.conversation_id + '">';
                            html += '<span class="icon is-small"><i class="mdi mdi-' + icon + '"></i></span>';
                            html += '<span>' + escapeHtml(label) + '</span>';
                            html += '</a>';
                        }
                    }
                    container.innerHTML = html;
                }

                function renderTaskSections() {
                    var status = window.__agentStatus || null;
                    var questions = (status && status.questions) || [];
                    var blocked = (status && status.blocked) || [];
                    var qContainer = document.getElementById('agent-question-entries');
                    if (qContainer) {
                        var qHtml = '';
                        if (questions.length > 0) {
                            qHtml += '<hr class="dropdown-divider">';
                            qHtml += '<div class="dropdown-item has-text-weight-semibold is-size-7 has-text-cyan">' +
                                '<span class="icon is-small"><i class="mdi mdi-help-circle-outline"></i></span>' +
                                '<span>Needs your input</span></div>';
                            for (var i = 0; i < questions.length; i++) {
                                qHtml += taskEntryHtml(questions[i], 'help-circle-outline');
                            }
                        }
                        qContainer.innerHTML = qHtml;
                    }
                    var bContainer = document.getElementById('agent-blocked-entries');
                    if (bContainer) {
                        var bHtml = '';
                        if (blocked.length > 0) {
                            bHtml += '<hr class="dropdown-divider">';
                            bHtml += '<div class="dropdown-item has-text-weight-semibold is-size-7 has-text-danger">' +
                                '<span class="icon is-small"><i class="mdi mdi-alert-circle-outline"></i></span>' +
                                '<span>Blocked</span></div>';
                            for (var j = 0; j < blocked.length; j++) {
                                bHtml += taskEntryHtml(blocked[j], 'alert-circle-outline');
                            }
                        }
                        bContainer.innerHTML = bHtml;
                    }
                }

                function updateAgentCount(count) {
                    var el = document.getElementById('agent-count');
                    if (el) el.textContent = count + ' Agents';
                }

                function updateStyling(status) {
                    if (!dd) return;
                    var agents = (status && status.agents) || [];
                    var questions = (status && status.questions) || [];
                    var blocked = (status && status.blocked) || [];
                    var hasRunning = agents.length > 0;
                    var hasQuestions = questions.length > 0;
                    var hasBlocked = blocked.length > 0;
                    // Blocked trumps question; both coexist with a running count.
                    dd.classList.toggle('has-blocked-tasks', hasBlocked);
                    dd.classList.toggle('has-question-tasks', hasQuestions && !hasBlocked);
                    dd.classList.toggle('has-running-agents', hasRunning);
                    var msgIcon = document.getElementById('agent-message-icon');
                    if (msgIcon) msgIcon.classList.toggle('agent-pulse', hasRunning);
                }

                function applyStatus(status) {
                    window.__agentStatus = status;
                    renderAgentEntries(status.agents || []);
                    updateAgentCount((status.agents || []).length);
                    renderTaskSections();
                    updateStyling(status);
                }

                function fetchGlobalStatus() {
                    window.apiCall('/api/tasks/agent-status')
                        .then(function(r) { return r.ok ? r.json() : Promise.reject(r.status); })
                        .then(applyStatus)
                        .catch(function() {});
                }

                function handleAgentStatusEvent(msg) {
                    if (msg.event_type === 'agent_status') {
                        fetchGlobalStatus();
                    }
                }

                var agentStatusSubscribed = false;

                function subscribeToAgentStatus() {
                    if (agentStatusSubscribed) return;
                    if (window.OfmWS) {
                        agentStatusSubscribed = true;
                        window.OfmWS.subscribe(
                            { kind: 'system', id: 0 },
                            handleAgentStatusEvent
                        );
                    }
                }

                if (window.OfmWS && window.OfmWS.status === 'connected') {
                    subscribeToAgentStatus();
                    fetchGlobalStatus();
                }
                document.addEventListener('ws-status-changed', function(ev) {
                    if (ev.detail.status === 'connected') {
                        subscribeToAgentStatus();
                        fetchGlobalStatus();
                    }
                });

                // Safety net: re-sync so a dropped event can never leave the
                // button stale. Server activity is still the driver.
                setInterval(function() {
                    if (window.OfmWS && window.OfmWS.status === 'connected') {
                        fetchGlobalStatus();
                    }
                }, 30000);
            })();"#}
        </script>
        <style>
            {r#"#agent-dropdown-trigger.button {
                background-color: var(--bulma-black) !important;
                color: var(--bulma-white) !important;
                border: 1px solid var(--bulma-white) !important;
            }
            #agent-dropdown-trigger.button:hover {
                background-color: var(--bulma-black) !important;
                color: var(--bulma-white) !important;
            }
            #agent-dropdown-trigger-2.button {
                background-color: var(--bulma-black) !important;
                color: var(--bulma-white) !important;
                border: 1px solid var(--bulma-white) !important;
            }
            #agent-dropdown-trigger-2.button:hover {
                background-color: var(--bulma-black) !important;
                color: var(--bulma-white) !important;
            }
            #ws-status-entry { cursor: default; }
            #agent-dropdown-menu .dropdown-content {
                background: var(--bulma-white-bis);
                color: var(--bulma-grey-darker);
                border: 1px solid var(--bulma-grey);
                border-radius: 3px;
            }
            /* >= 1 open question tasks: tint the whole trigger cyan */
            #agent-dropdown.has-question-tasks #agent-dropdown-trigger.button,
            #agent-dropdown.has-question-tasks #agent-dropdown-trigger-2.button {
                background-color: var(--bulma-cyan) !important;
                border-color: var(--bulma-cyan) !important;
            }
            /* >= 1 blocked tasks: primary color, trumping every other rule */
            #agent-dropdown.has-blocked-tasks #agent-dropdown-trigger.button,
            #agent-dropdown.has-blocked-tasks #agent-dropdown-trigger-2.button {
                background-color: var(--bulma-primary) !important;
                border-color: var(--bulma-primary) !important;
            }
            /* Pulse the message icon once every 15s while >= 1 agent runs */
            #agent-dropdown #agent-message-icon.agent-pulse {
                animation: agent-pulse 15s ease-in-out infinite;
            }
            @keyframes agent-pulse {
                0%, 100% { opacity: 1; transform: scale(1); }
                2% { opacity: 0.25; transform: scale(0.8); }
                7% { opacity: 1; transform: scale(1); }
            }"#}
        </style>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::schema::AgentType;
    use uuid::Uuid;

    fn make_agent(
        agent_type: AgentType,
        project_title: &str,
        task_title: &str,
        conv_name: Option<&str>,
    ) -> ActiveAgent {
        ActiveAgent {
            agent_type,
            project_id: 1,
            project_title: project_title.to_string(),
            task_id: 1,
            task_title: task_title.to_string(),
            conversation_id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap(),
            conversation_name: conv_name.map(|s| s.to_string()),
        }
    }

    #[test]
    fn test_dropdown_renders_connection_status_entry() {
        let html = leptos::view! { <AgentDropdown active_agents=Vec::new() /> }.to_html();
        assert!(
            html.contains("ws-status-entry"),
            "should have ws-status-entry id"
        );
        assert!(html.contains("ws-icon"), "should have ws-icon id");
        assert!(html.contains("ws-label"), "should have ws-label id");
        assert!(
            html.contains("ws-last-payload"),
            "should have ws-last-payload id"
        );
        assert!(
            html.contains("ws-status-changed"),
            "should listen for ws-status-changed"
        );
        assert!(
            html.contains("ws-payload-received"),
            "should listen for ws-payload-received"
        );
    }

    #[test]
    fn test_dropdown_zero_agents_shows_count() {
        let html = leptos::view! { <AgentDropdown active_agents=Vec::new() /> }.to_html();
        assert!(html.contains("0 Agents"), "should show 0 Agents");
        assert!(!html.contains("disabled"), "trigger should not be disabled");
    }

    #[test]
    fn test_dropdown_renders_agent_entry() {
        let agents = vec![make_agent(
            AgentType::Implementation,
            "Proj",
            "Task",
            Some("Conv"),
        )];
        let html = leptos::view! { <AgentDropdown active_agents=agents /> }.to_html();
        assert!(html.contains("1 Agents"), "should show 1 Agents");
        assert!(
            html.contains("/webapp/projects/1/tasks/1/chat/550e8400-e29b-41d4-a716-446655440000"),
            "should have correct href"
        );
        assert!(
            html.contains("mdi-code-tags"),
            "should have agent type icon"
        );
        assert!(
            html.contains("Proj/Task: Conv"),
            "should show truncated labels"
        );
    }

    #[test]
    fn test_dropdown_renders_multiple_entries() {
        let agents = vec![
            make_agent(AgentType::Planification, "P1", "T1", Some("C1")),
            make_agent(AgentType::Review, "P2", "T2", Some("C2")),
        ];
        let html = leptos::view! { <AgentDropdown active_agents=agents /> }.to_html();
        assert!(html.contains("2 Agents"), "should show 2 Agents");
        assert!(html.contains("P1/T1: C1"));
        assert!(html.contains("P2/T2: C2"));
    }

    #[test]
    fn test_dropdown_uses_agent_type_icon() {
        for (agent_type, expected_icon) in [
            (AgentType::Planification, "mdi-file-document-outline"),
            (AgentType::Implementation, "mdi-code-tags"),
            (AgentType::Refinement, "mdi-creation-outline"),
            (AgentType::Review, "mdi-checkbox-marked-circle-outline"),
            (AgentType::Pr, "mdi-source-branch-plus"),
            (AgentType::Yolo, "mdi-rocket"),
        ] {
            let agents = vec![make_agent(agent_type.clone(), "P", "T", Some("C"))];
            let html = leptos::view! { <AgentDropdown active_agents=agents /> }.to_html();
            assert!(
                html.contains(expected_icon),
                "missing icon {expected_icon} for {agent_type:?}"
            );
        }
    }

    #[test]
    fn test_dropdown_truncates_long_titles() {
        let long_title = "A very long title that exceeds twenty four characters";
        let agents = vec![make_agent(
            AgentType::Implementation,
            long_title,
            long_title,
            Some(long_title),
        )];
        let html = leptos::view! { <AgentDropdown active_agents=agents /> }.to_html();
        let truncated = title_truncate(long_title);
        assert!(
            html.contains(&truncated),
            "should truncate long titles to 24 chars + ellipsis"
        );
        assert!(
            !html.contains(long_title),
            "should not contain the full untruncated title"
        );
    }

    #[test]
    fn test_dropdown_fallback_name_when_conv_name_none() {
        let agents = vec![make_agent(AgentType::Implementation, "P", "T", None)];
        let html = leptos::view! { <AgentDropdown active_agents=agents /> }.to_html();
        assert!(html.contains("Unnamed"), "should use 'Unnamed' as fallback");
    }

    #[test]
    fn test_dropdown_has_divider() {
        let agents = vec![make_agent(AgentType::Implementation, "P", "T", Some("C"))];
        let html = leptos::view! { <AgentDropdown active_agents=agents /> }.to_html();
        assert!(
            html.contains("dropdown-divider"),
            "should have dropdown divider"
        );
    }

    #[test]
    fn test_dropdown_fetches_global_status_endpoint() {
        let html = leptos::view! { <AgentDropdown active_agents=Vec::new() /> }.to_html();
        assert!(
            html.contains("/api/tasks/agent-status"),
            "should fetch the aggregate agent-status endpoint"
        );
        assert!(
            html.contains("agent_status"),
            "should react to agent_status events"
        );
        assert!(
            html.contains("{ kind: 'system', id: 0 }"),
            "should subscribe to the System topic"
        );
    }

    #[test]
    fn test_dropdown_has_question_and_blocked_sections() {
        let html = leptos::view! { <AgentDropdown active_agents=Vec::new() /> }.to_html();
        assert!(
            html.contains("agent-question-entries"),
            "should have a question tasks section"
        );
        assert!(
            html.contains("agent-blocked-entries"),
            "should have a blocked tasks section"
        );
        assert!(
            html.contains("Needs your input"),
            "should label open-question tasks"
        );
        assert!(html.contains("Blocked"), "should label blocked tasks");
    }

    #[test]
    fn test_dropdown_pulse_icon_styling() {
        let html = leptos::view! { <AgentDropdown active_agents=Vec::new() /> }.to_html();
        assert!(
            html.contains("agent-message-icon"),
            "message-outline icon should carry an id for the pulse"
        );
        assert!(
            html.contains("agent-pulse"),
            "should define the pulse animation"
        );
        assert!(
            html.contains("has-question-tasks"),
            "should style question state"
        );
        assert!(
            html.contains("has-blocked-tasks"),
            "should style blocked state"
        );
        assert!(
            html.contains("--bulma-cyan"),
            "question state should use the cyan palette"
        );
    }
}
