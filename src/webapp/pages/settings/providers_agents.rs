use leptos::prelude::*;

use crate::webapp::components::agent_model_select::AgentModelSelect;
use crate::webapp::components::config_body_editor::ConfigBodyEditor;
use crate::webapp::components::settings_sidebar::{
    SettingsSection, SettingsSidebar, SettingsSubPage,
};

pub fn render(active: SettingsSubPage) -> String {
    let sidebar = leptos::view! {
        <SettingsSidebar section=SettingsSection::ProvidersAgents active />
    }
    .to_html();

    let (pane, js) = match active {
        SettingsSubPage::ModelConfig => (
            leptos::view! { <div class="box"><ConfigBodyEditor/></div> }.to_html(),
            CONFIG_JS,
        ),
        SettingsSubPage::AgentSettings => (
            leptos::view! { <div class="box"><AgentModelSelect/></div> }.to_html(),
            AGENT_MODELS_JS,
        ),
        _ => unreachable!("providers-agents page only renders its own sub-pages"),
    };

    format!(
        r#"<section class="section">
            <h2 class="title is-3">
                <span class="icon is-medium"><i class="mdi mdi-cog-outline"></i></span>
                "Providers & Agents"
            </h2>
            <div class="columns">
                <div class="column is-one-quarter">
                    {}
                </div>
                <div class="column">
                    {}
                </div>
            </div>
        </section>
        <script>{}</script>"#,
        sidebar, pane, js
    )
}

const CONFIG_JS: &str = r#"
window.__ACCESS_TOKEN__ = '';

document.addEventListener('DOMContentLoaded', function() {
    loadConfigList();
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
                if (cfg.id === '00000000-0000-0000-0000-00000000dead') return;
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
                document.getElementById('new-config-harness').value = 'opencode';
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
"#;

const AGENT_MODELS_JS: &str = r#"
window.__ACCESS_TOKEN__ = '';

function escapeHtml(str) {
    var div = document.createElement('div');
    div.appendChild(document.createTextNode(str));
    return div.innerHTML;
}

document.addEventListener('DOMContentLoaded', function() {
    loadAgentModels();
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
        var agents = ['planification', 'implementation', 'refinement', 'review', 'pr', 'yolo', 'conversation_title'];
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
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_providers_agents_renders_model_config_landing() {
        let html = render(SettingsSubPage::ModelConfig);
        assert!(html.contains("menu"));
        assert!(html.contains("Model Configurations"));
        assert!(html.contains("config-list"));
        assert!(html.contains("__ACCESS_TOKEN__"));
        assert!(html.contains("loadConfigList"));
        assert!(html.contains("Agent Settings"));
        assert!(!html.contains("agent-model-tbody"));
    }

    #[test]
    fn test_providers_agents_renders_agent_settings() {
        let html = render(SettingsSubPage::AgentSettings);
        assert!(html.contains("menu"));
        assert!(html.contains("Agent Settings"));
        assert!(html.contains("agent-model-tbody"));
        assert!(html.contains("loadAgentModels"));
        assert!(html.contains("function escapeHtml(str)"));
        assert!(!html.contains("config-list"));
    }
}
