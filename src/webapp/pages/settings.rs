use leptos::prelude::*;

use crate::webapp::components::agent_model_select::AgentModelSelect;
use crate::webapp::components::api_key_manager::ApiKeyManager;
use crate::webapp::components::config_body_editor::ConfigBodyEditor;

#[component]
pub fn SettingsPage(access_token: String) -> impl IntoView {
    view! {
        <main>
            <script>
                {format!(
                    "window.__ACCESS_TOKEN__ = '{}';",
                    access_token.replace('\'', "\\'"),
                )}
            </script>
            <div class="settings-page">
                <h2>"Settings"</h2>
                <div class="tabs">
                    <button class="tab active" data-tab="config-body">"Model Configurations"</button>
                    <button class="tab" data-tab="agent-models">"Agent Settings"</button>
                    <button class="tab" data-tab="api-keys">"API Keys"</button>
                </div>
                <div class="tab-content" id="tab-content">
                    <div id="tab-config-body" class="tab-pane active">
                        <ConfigBodyEditor/>
                    </div>
                    <div id="tab-agent-models" class="tab-pane">
                        <AgentModelSelect/>
                    </div>
                    <div id="tab-api-keys" class="tab-pane">
                        <ApiKeyManager/>
                    </div>
                </div>
            </div>
            <script>
                {r#"
document.addEventListener('DOMContentLoaded', function() {
    var tabs = document.querySelectorAll('.tab');
    var panes = {
        'config-body': document.getElementById('tab-config-body'),
        'agent-models': document.getElementById('tab-agent-models'),
        'api-keys': document.getElementById('tab-api-keys')
    };

    tabs.forEach(function(tab) {
        tab.addEventListener('click', function() {
            tabs.forEach(function(t) { t.classList.remove('active'); });
            this.classList.add('active');
            var tabName = this.dataset.tab;
            Object.keys(panes).forEach(function(k) {
                panes[k].style.display = (k === tabName) ? 'block' : 'none';
            });
        });
    });

    // Show first tab by default
    Object.keys(panes).forEach(function(k) {
        panes[k].style.display = (k === 'config-body') ? 'block' : 'none';
    });

    // Load config-body list
    loadConfigList();

    // Load agent models
    loadAgentModels();

    // Check API key status
    checkApiKey();
});

function loadConfigList() {
    var list = document.getElementById('config-list');
    if (!list) return;
    apiCall('/api/settings/config-body')
        .then(function(r) { return r.json(); })
        .then(function(data) {
            if (data.length === 0) {
                list.innerHTML = '<p>No configurations yet. Add one below.</p>';
                return;
            }
            var html = '<table class="config-table"><thead><tr><th>Name</th><th>Harness</th><th>Actions</th></tr></thead><tbody>';
            data.forEach(function(cfg) {
                html += '<tr>';
                html += '<td>' + escapeHtml(cfg.name) + '</td>';
                html += '<td>' + escapeHtml(cfg.harness) + '</td>';
                html += '<td><button class="btn btn-sm" onclick="editConfig(\'' + cfg.id + '\')">Edit</button> ';
                html += '<button class="btn btn-sm btn-danger" onclick="deleteConfig(\'' + cfg.id + '\')">Delete</button></td>';
                html += '</tr>';
            });
            html += '</tbody></table>';
            list.innerHTML = html;
        })
        .catch(function(err) {
            list.innerHTML = '<p class="error">Failed to load configurations: ' + err + '</p>';
        });
}

function escapeHtml(str) {
    var div = document.createElement('div');
    div.appendChild(document.createTextNode(str));
    return div.innerHTML;
}

document.addEventListener('DOMContentLoaded', function() {
    var btn = document.getElementById('btn-add-config');
    if (btn) {
        btn.addEventListener('click', function() {
            var name = document.getElementById('new-config-name').value.trim();
            var harness = document.getElementById('new-config-harness').value.trim();
            var configBody = document.getElementById('new-config-body').value.trim();
            if (!name || !configBody) {
                alert('Name and Config Body are required.');
                return;
            }
            apiCall('/api/settings/config-body', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ name: name, config_body: configBody, harness: harness })
            })
            .then(function(r) {
                if (!r.ok) throw new Error('Failed to save');
                return r.json();
            })
            .then(function() {
                document.getElementById('new-config-name').value = '';
                document.getElementById('new-config-harness').value = '';
                document.getElementById('new-config-body').value = '';
                loadConfigList();
            })
            .catch(function(err) {
                alert('Error: ' + err.message);
            });
        });
    }
});

window.deleteConfig = function(id) {
    if (!confirm('Delete this configuration?')) return;
    apiCall('/api/settings/config-body/' + id, { method: 'DELETE' })
        .then(function(r) {
            if (!r.ok) throw new Error('Delete failed');
            loadConfigList();
        })
        .catch(function(err) {
            alert('Error: ' + err.message);
        });
};

window.editConfig = function(id) {
    var name = prompt('New name:');
    if (!name) return;
    var body = prompt('New config body (YAML or JSON):');
    if (!body) return;
    var harness = prompt('New harness:') || '';
    apiCall('/api/settings/config-body/' + id, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ name: name, config_body: body, harness: harness })
    })
    .then(function(r) {
        if (!r.ok) throw new Error('Update failed');
        loadConfigList();
    })
    .catch(function(err) {
        alert('Error: ' + err.message);
    });
};

function loadAgentModels() {
    var tbody = document.getElementById('agent-model-tbody');
    if (!tbody) return;
    apiCall('/api/settings/agent-models')
        .then(function(r) { return r.json(); })
        .then(function(data) {
            var agents = ['planification', 'implementation', 'refinement', 'review', 'pr', 'yolo'];
            var html = '';
            agents.forEach(function(agent) {
                var setting = data[agent] || {};
                html += '<tr>';
                html += '<td>' + agent + '</td>';
                html += '<td><input type="text" class="model-input" data-agent="' + agent + '" value="' + (setting.model || '') + '" placeholder="model name"/></td>';
                html += '<td><select class="effort-select" data-agent="' + agent + '">';
                ['auto', 'low', 'medium', 'high'].forEach(function(eff) {
                    var selected = (setting.effort === eff) ? ' selected' : '';
                    html += '<option value="' + eff + '"' + selected + '>' + eff + '</option>';
                });
                html += '</select></td>';
                html += '</tr>';
            });
            tbody.innerHTML = html;
        })
        .catch(function(err) {
            tbody.innerHTML = '<tr><td colspan="3" class="error">Failed to load: ' + err + '</td></tr>';
        });
}

document.addEventListener('DOMContentLoaded', function() {
    var btn = document.getElementById('btn-save-agent-models');
    if (btn) {
        btn.addEventListener('click', function() {
            var models = {};
            document.querySelectorAll('.model-input').forEach(function(input) {
                var agent = input.dataset.agent;
                if (!models[agent]) models[agent] = {};
                models[agent].model = input.value || null;
            });
            document.querySelectorAll('.effort-select').forEach(function(select) {
                var agent = select.dataset.agent;
                if (!models[agent]) models[agent] = {};
                models[agent].effort = select.value;
            });
            apiCall('/api/settings/agent-models', {
                method: 'PUT',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify(models)
            })
            .then(function(r) {
                if (!r.ok) throw new Error('Failed to save');
                return r.json();
            })
            .then(function() {
                alert('Agent settings saved.');
            })
            .catch(function(err) {
                alert('Error: ' + err.message);
            });
        });
    }
});

function checkApiKey() {
    var display = document.getElementById('api-key-display');
    var empty = document.getElementById('api-key-empty');
    var generateBtn = document.getElementById('btn-generate-key');
    var revokeBtn = document.getElementById('btn-revoke-key');
    // We check by trying to get current key status from the auth endpoint
    // For simplicity, start with generate-only mode
}

document.addEventListener('DOMContentLoaded', function() {
    var generateBtn = document.getElementById('btn-generate-key');
    var revokeBtn = document.getElementById('btn-revoke-key');
    var display = document.getElementById('api-key-display');
    var keyValue = document.getElementById('api-key-value');
    var empty = document.getElementById('api-key-empty');

    if (generateBtn) {
        generateBtn.addEventListener('click', function() {
            apiCall('/api/auth/api-key', { method: 'POST' })
                .then(function(r) { return r.json(); })
                .then(function(data) {
                    if (data.api_key) {
                        keyValue.textContent = data.api_key;
                        display.style.display = 'block';
                        empty.style.display = 'none';
                        generateBtn.style.display = 'none';
                        revokeBtn.style.display = 'inline-block';
                    }
                })
                .catch(function(err) {
                    alert('Error generating key: ' + err.message);
                });
        });
    }

    if (revokeBtn) {
        revokeBtn.addEventListener('click', function() {
            apiCall('/api/auth/api-key', { method: 'DELETE' })
                .then(function(r) {
                    if (!r.ok) throw new Error('Revoke failed');
                    display.style.display = 'none';
                    empty.style.display = 'block';
                    generateBtn.style.display = 'inline-block';
                    revokeBtn.style.display = 'none';
                })
                .catch(function(err) {
                    alert('Error: ' + err.message);
                });
        });
    }

    var copyBtn = document.getElementById('btn-copy-key');
    if (copyBtn) {
        copyBtn.addEventListener('click', function() {
            var text = keyValue.textContent;
            navigator.clipboard.writeText(text).then(function() {
                alert('API key copied to clipboard.');
            }).catch(function() {
                alert('Failed to copy. Please copy manually.');
            });
        });
    }
});
                "#}
            </script>
        </main>
    }
}
