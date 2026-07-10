use leptos::prelude::*;

use crate::webapp::components::agent_model_select::AgentModelSelect;
use crate::webapp::components::api_key_manager::ApiKeyManager;
use crate::webapp::components::config_body_editor::ConfigBodyEditor;

#[component]
pub fn SettingsPage(access_token: String) -> impl IntoView {
    view! {
        <div class="section">
            <h2 class="title is-3">
                <span class="icon is-medium"><i class="mdi mdi-cog-outline"></i></span>
                " Settings"
            </h2>
            <div class="tabs is-boxed is-medium">
                <ul>
                    <li class="is-active" data-tab="config-body">
                        <a>
                            <span class="icon is-small"><i class="mdi mdi-cog-outline"></i></span>
                            <span>"Model Configurations"</span>
                        </a>
                    </li>
                    <li data-tab="agent-models">
                        <a>
                            <span class="icon is-small"><i class="mdi mdi-robot"></i></span>
                            <span>"Agent Settings"</span>
                        </a>
                    </li>
                    <li data-tab="api-keys">
                        <a>
                            <span class="icon is-small"><i class="mdi mdi-key-variant"></i></span>
                            <span>"API Keys"</span>
                        </a>
                    </li>
                </ul>
            </div>
            <div class="tab-content" id="tab-content">
                <div id="tab-config-body" class="tab-pane box">
                    <ConfigBodyEditor/>
                </div>
                <div id="tab-agent-models" class="tab-pane box" style="display:none">
                    <AgentModelSelect/>
                </div>
                <div id="tab-api-keys" class="tab-pane box" style="display:none">
                    <ApiKeyManager/>
                </div>
            </div>
        </div>
        <script>
            {format!(
                "window.__ACCESS_TOKEN__ = '{}';",
                access_token.replace('\'', "\\'"),
            )}
        </script>
        <script>
            {r#"
document.addEventListener('DOMContentLoaded', function() {
    var tabs = document.querySelectorAll('.tabs li');
    var panes = {
        'config-body': document.getElementById('tab-config-body'),
        'agent-models': document.getElementById('tab-agent-models'),
        'api-keys': document.getElementById('tab-api-keys')
    };

    tabs.forEach(function(tab) {
        tab.addEventListener('click', function() {
            tabs.forEach(function(t) { t.classList.remove('is-active'); });
            this.classList.add('is-active');
            var tabName = this.dataset.tab;
            Object.keys(panes).forEach(function(k) {
                panes[k].style.display = (k === tabName) ? 'block' : 'none';
            });
        });
    });

    loadConfigList();
    loadAgentModels();
    checkApiKey();
});

window.__CONFIGS__ = [];

function loadConfigList() {
    var list = document.getElementById('config-list');
    if (!list) return;
    apiCall('/api/settings/config-body')
        .then(function(r) { return r.json(); })
        .then(function(data) {
            window.__CONFIGS__ = data;
            if (data.length === 0) {
                list.innerHTML = '<p>No configurations yet. Add one below.</p>';
                return;
            }
            var html = '<table class="table is-fullwidth is-hoverable"><thead><tr><th>Name</th><th>Harness</th><th>Actions</th></tr></thead><tbody>';
            data.forEach(function(cfg) {
                html += '<tr>';
                html += '<td>' + escapeHtml(cfg.name) + '</td>';
                html += '<td>' + escapeHtml(cfg.harness) + '</td>';
                html += '<td><button class="button is-small" onclick="editConfig(\'' + cfg.id + '\')"><span class="icon is-small"><i class="mdi mdi-pencil"></i></span><span>Edit</span></button> ';
                html += '<button class="button is-small is-danger" onclick="deleteConfig(\'' + cfg.id + '\')"><span class="icon is-small"><i class="mdi mdi-delete"></i></span><span>Delete</span></button></td>';
                html += '</tr>';
            });
            html += '</tbody></table>';
            list.innerHTML = html;
        })
        .catch(function(err) {
            list.innerHTML = '<p class="has-text-danger">Failed to load configurations: ' + err + '</p>';
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
            var harness = document.getElementById('new-config-harness').value;
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
                document.getElementById('new-config-harness').value = 'oh-my-pi';
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
    var cfg = window.__CONFIGS__.find(function(c) { return c.id === id; });
    if (!cfg) {
        alert('Configuration not found');
        return;
    }
    document.getElementById('edit-config-id').value = id;
    document.getElementById('edit-config-name').value = cfg.name;
    document.getElementById('edit-config-harness').value = cfg.harness;
    document.getElementById('edit-config-body').value = cfg.config_body;
    document.getElementById('edit-config-modal').classList.add('is-active');
};

document.addEventListener('DOMContentLoaded', function() {
    var saveBtn = document.getElementById('btn-save-edit-config');
    var cancelBtn = document.getElementById('btn-cancel-edit-config');
    var closeBtn = document.getElementById('btn-close-edit-modal');
    var modalBg = document.querySelector('#edit-config-modal .modal-background');

    function closeEditModal() {
        document.getElementById('edit-config-modal').classList.remove('is-active');
    }

    if (saveBtn) {
        saveBtn.addEventListener('click', function() {
            var id = document.getElementById('edit-config-id').value;
            var name = document.getElementById('edit-config-name').value.trim();
            var harness = document.getElementById('edit-config-harness').value;
            var configBody = document.getElementById('edit-config-body').value.trim();
            if (!name || !configBody) {
                alert('Name and Config Body are required.');
                return;
            }
            apiCall('/api/settings/config-body/' + id, {
                method: 'PUT',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ name: name, config_body: configBody, harness: harness })
            })
            .then(function(r) {
                if (!r.ok) throw new Error('Update failed');
                closeEditModal();
                loadConfigList();
            })
            .catch(function(err) {
                alert('Error: ' + err.message);
            });
        });
    }

    if (cancelBtn) cancelBtn.addEventListener('click', closeEditModal);
    if (closeBtn) closeBtn.addEventListener('click', closeEditModal);
    if (modalBg) modalBg.addEventListener('click', closeEditModal);
});

function loadAgentModels() {
    var tbody = document.getElementById('agent-model-tbody');
    if (!tbody) return;
    Promise.all([
        apiCall('/api/settings/agent-models').then(function(r) { return r.json(); }),
        apiCall('/api/settings/config-body').then(function(r) { return r.json(); })
    ])
    .then(function(results) {
        var data = results[0];
        var configs = results[1];
        var agents = ['planification', 'implementation', 'refinement', 'review', 'pr', 'yolo'];
        var html = '';
        agents.forEach(function(agent) {
            var setting = data[agent] || {};
            html += '<tr>';
            html += '<td>' + agent + '</td>';
            html += '<td>';
            html += '<select class="select" data-agent="' + agent + '" data-model-config="true">';
            html += '<option value="">-- Select config --</option>';
            configs.forEach(function(cfg) {
                var selected = (cfg.id === setting.model_config_id) ? ' selected' : '';
                html += '<option value="' + cfg.id + '"' + selected + '>' + escapeHtml(cfg.name) + '</option>';
            });
            html += '</select>';
            html += '<div style="margin-top:0.3rem"><input type="text" class="input" data-agent="' + agent + '" data-model-name="true" value="' + (setting.model || '') + '" placeholder="model name (e.g. gpt-4)"/></div>';
            html += '</td>';
            html += '<td><select class="select" data-agent="' + agent + '">';
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
        tbody.innerHTML = '<tr><td colspan="3" class="has-text-danger">Failed to load: ' + err + '</td></tr>';
    });
}

document.addEventListener('DOMContentLoaded', function() {
    var btn = document.getElementById('btn-save-agent-models');
    if (btn) {
        btn.addEventListener('click', function() {
            var models = {};
            document.querySelectorAll('select[data-model-config="true"]').forEach(function(select) {
                var agent = select.dataset.agent;
                if (!models[agent]) models[agent] = {};
                models[agent].model_config_id = select.value || null;
            });
            document.querySelectorAll('input[data-model-name="true"]').forEach(function(input) {
                var agent = input.dataset.agent;
                if (!models[agent]) models[agent] = {};
                models[agent].model = input.value || null;
            });
            document.querySelectorAll('tbody#agent-model-tbody td select:not([data-model-config])').forEach(function(select) {
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
    }
}
