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
                    <li data-tab="export">
                        <a>
                            <span class="icon is-small"><i class="mdi mdi-export-variant"></i></span>
                            <span>"Export"</span>
                        </a>
                    </li>
                    <li data-tab="import">
                        <a>
                            <span class="icon is-small"><i class="mdi mdi-import"></i></span>
                            <span>"Import"</span>
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
                <div id="tab-export" class="tab-pane box" style="display:none">
                    <h3 class="title is-4">
                        <span class="icon is-medium"><i class="mdi mdi-export-variant"></i></span>
                        "Export Projects"
                    </h3>
                    <p class="mb-3">
                        "Export selected projects and their tasks as a JSON file. The export includes project metadata, task titles, descriptions (from markdown doc files), status, and conversation metadata (IDs, models, effort, timestamps). Conversation messages are not included."
                    </p>
                    <div class="field">
                        <div class="control">
                            <button class="button is-small" id="export-select-all">"Select All"</button>
                            <button class="button is-small" id="export-deselect-all">"Deselect All"</button>
                        </div>
                    </div>
                    <div id="export-project-list" class="mb-4">
                        <p class="has-text-grey">"Loading projects..."</p>
                    </div>
                    <button class="button is-small is-primary" id="btn-export">
                        <span class="icon is-small"><i class="mdi mdi-download"></i></span>
                        <span>"Export Selected"</span>
                    </button>
                </div>
                <div id="tab-import" class="tab-pane box" style="display:none">
                    <h3 class="title is-4">
                        <span class="icon is-medium"><i class="mdi mdi-import"></i></span>
                        "Import Projects"
                    </h3>
                    <p class="mb-3">
                        "Upload a previously exported OFM JSON file to import projects and tasks. You can choose to create new projects or add tasks to existing ones."
                    </p>
                    <div class="field">
                        <div class="file is-boxed">
                            <label class="file-label">
                                <input class="file-input" type="file" accept=".json" id="import-file-input"/>
                                <span class="file-cta">
                                    <span class="file-icon"><i class="mdi mdi-upload"></i></span>
                                    <span class="file-label">"Choose a JSON file..."</span>
                                </span>
                            </label>
                        </div>
                    </div>
                    <div id="import-error" class="notification is-danger" style="display:none"></div>
                    <div id="import-preview" style="display:none">
                        <h4 class="title is-5">"Preview"</h4>
                        <p class="mb-3 has-text-grey">"The following projects were found in the import file. Enable the ones you want to import and choose an import target for each."</p>
                        <div id="import-project-cards"></div>
                        <button class="button is-small is-primary" id="btn-import">
                            <span class="icon is-small" id="import-spinner" style="display:none"><i class="mdi mdi-loading mdi-spin"></i></span>
                            <span>"Import"</span>
                        </button>
                    </div>
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
        'api-keys': document.getElementById('tab-api-keys'),
        'export': document.getElementById('tab-export'),
        'import': document.getElementById('tab-import')
    };

    tabs.forEach(function(tab) {
        tab.addEventListener('click', function() {
            tabs.forEach(function(t) { t.classList.remove('is-active'); });
            this.classList.add('is-active');
            var tabName = this.dataset.tab;
            Object.keys(panes).forEach(function(k) {
                panes[k].style.display = (k === tabName) ? 'block' : 'none';
            });
            if (tabName === 'export') loadExportTab();
        });
    });

    loadConfigList();
    loadAgentModels();
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

// ── Export tab ──────────────────────────────────────────────────────────

function loadExportTab() {
    var list = document.getElementById('export-project-list');
    if (!list) return;
    apiCall('/api/projects')
        .then(function(r) { return r.json(); })
        .then(function(data) {
            if (data.length === 0) {
                list.innerHTML = '<p class="has-text-grey">No projects available to export.</p>';
                return;
            }
            var html = '';
            data.forEach(function(proj) {
                html += '<div class="field">';
                html += '<label class="checkbox">';
                html += '<input type="checkbox" class="export-project-cb" value="' + proj.id + '" checked> ';
                html += escapeHtml(proj.name);
                html += ' <span class="has-text-grey">(' + escapeHtml(proj.repo_folder_path) + ')</span>';
                html += '</label>';
                html += '</div>';
            });
            list.innerHTML = html;
        })
        .catch(function(err) {
            list.innerHTML = '<p class="has-text-danger">Failed to load projects: ' + err + '</p>';
        });
}

document.addEventListener('DOMContentLoaded', function() {
    var selectAllBtn = document.getElementById('export-select-all');
    var deselectAllBtn = document.getElementById('export-deselect-all');
    if (selectAllBtn) {
        selectAllBtn.addEventListener('click', function() {
            document.querySelectorAll('.export-project-cb').forEach(function(cb) { cb.checked = true; });
        });
    }
    if (deselectAllBtn) {
        deselectAllBtn.addEventListener('click', function() {
            document.querySelectorAll('.export-project-cb').forEach(function(cb) { cb.checked = false; });
        });
    }

    var exportBtn = document.getElementById('btn-export');
    if (exportBtn) {
        exportBtn.addEventListener('click', function() {
            var checked = [];
            document.querySelectorAll('.export-project-cb:checked').forEach(function(cb) {
                checked.push(cb.value);
            });
            if (checked.length === 0) {
                alert('Please select at least one project to export.');
                return;
            }
            var url = '/api/settings/export?project_ids=' + checked.join(',');
            apiCall(url)
                .then(function(r) {
                    if (!r.ok) throw new Error('Export failed');
                    return r.json();
                })
                .then(function(data) {
                    var blob = new Blob([JSON.stringify(data, null, 2)], { type: 'application/json' });
                    var date = new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19);
                    var filename = 'ofm-export-' + date + '.json';
                    var a = document.createElement('a');
                    a.href = URL.createObjectURL(blob);
                    a.download = filename;
                    document.body.appendChild(a);
                    a.click();
                    document.body.removeChild(a);
                    URL.revokeObjectURL(a.href);
                })
                .catch(function(err) {
                    alert('Export error: ' + err.message);
                });
        });
    }
});

// ── Import tab ──────────────────────────────────────────────────────────

var __importRawJson = null;

document.addEventListener('DOMContentLoaded', function() {
    var fileInput = document.getElementById('import-file-input');
    if (!fileInput) return;

    fileInput.addEventListener('change', function(e) {
        var file = e.target.files[0];
        if (!file) return;

        var reader = new FileReader();
        reader.onload = function(ev) {
            var content = ev.target.result;
            __importRawJson = content;

            var errorDiv = document.getElementById('import-error');
            var preview = document.getElementById('import-preview');
            var cards = document.getElementById('import-project-cards');

            errorDiv.style.display = 'none';
            preview.style.display = 'none';

            apiCall('/api/settings/import/preview', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: content
            })
            .then(function(r) {
                if (!r.ok) return r.json().then(function(errData) {
                    throw new Error(errData.error || 'Preview failed');
                });
                return r.json();
            })
            .then(function(data) {
                var html = '';
                data.projects.forEach(function(proj) {
                    html += '<div class="card mb-3" data-source-id="' + escapeHtml(proj.source_project_id) + '">';
                    html += '<div class="card-content">';
                    html += '<div class="level">';
                    html += '<div class="level-left">';
                    html += '<div class="level-item">';
                    html += '<label class="checkbox"><input type="checkbox" class="import-enabled-cb" checked> <strong>' + escapeHtml(proj.name) + '</strong></label>';
                    html += '</div>';
                    html += '<div class="level-item">';
                    html += '<span class="tag">' + proj.task_count + ' tasks</span>';
                    html += '</div>';
                    html += '</div>';
                    html += '</div>';
                    html += '<div class="field">';
                    html += '<label class="label">Import target</label>';
                    html += '<div class="select"><select class="import-target-select">';
                    html += '<option value="create_new">Create new project</option>';
                    html += '<option value="add_to_existing">Add to existing project</option>';
                    html += '</select></div>';
                    html += '</div>';
                    html += '<div class="import-create-fields">';
                    html += '<div class="field"><label class="label">Project name</label><div class="control"><input class="input import-name-input" type="text" value="' + escapeHtml(proj.name) + '"></div></div>';
                    html += '<div class="field"><label class="label">Repository path</label><div class="control"><input class="input import-path-input" type="text" value="' + escapeHtml(proj.repo_folder_path || '') + '"></div></div>';
                    html += '</div>';
                    html += '<div class="import-existing-fields" style="display:none">';
                    html += '<div class="field"><label class="label">Existing project</label><div class="control"><div class="select"><select class="import-existing-select"><option value="">Loading...</option></select></div></div></div>';
                    html += '</div>';
                    html += '</div>';
                    html += '</div>';
                });
                cards.innerHTML = html;
                preview.style.display = 'block';

                // Fetch existing projects for the "add to existing" dropdowns
                apiCall('/api/projects').then(function(r) { return r.json(); }).then(function(projects) {
                    document.querySelectorAll('.import-existing-select').forEach(function(sel) {
                        var html = '<option value="">-- Select project --</option>';
                        projects.forEach(function(p) {
                            html += '<option value="' + p.id + '">' + escapeHtml(p.name) + '</option>';
                        });
                        sel.innerHTML = html;
                    });
                }).catch(function() {});

                // Set up target type switching
                document.querySelectorAll('.import-target-select').forEach(function(sel) {
                    sel.addEventListener('change', function() {
                        var card = this.closest('.card');
                        var createFields = card.querySelector('.import-create-fields');
                        var existingFields = card.querySelector('.import-existing-fields');
                        if (this.value === 'create_new') {
                            createFields.style.display = 'block';
                            existingFields.style.display = 'none';
                        } else {
                            createFields.style.display = 'none';
                            existingFields.style.display = 'block';
                        }
                    });
                });

                // Set up enable/disable toggling
                document.querySelectorAll('.import-enabled-cb').forEach(function(cb) {
                    cb.addEventListener('change', function() {
                        var card = this.closest('.card');
                        var inputs = card.querySelectorAll('input, select');
                        inputs.forEach(function(inp) {
                            if (inp !== cb) inp.disabled = !cb.checked;
                        });
                    });
                });
            })
            .catch(function(err) {
                errorDiv.textContent = 'Error: ' + err.message;
                errorDiv.style.display = 'block';
            });
        };
        reader.readAsText(file);
    });

    var importBtn = document.getElementById('btn-import');
    if (importBtn) {
        importBtn.addEventListener('click', function() {
            if (!__importRawJson) {
                alert('No file loaded. Please select a file first.');
                return;
            }

            var imports = [];
            document.querySelectorAll('.card[data-source-id]').forEach(function(card) {
                var enabledCb = card.querySelector('.import-enabled-cb');
                if (!enabledCb || !enabledCb.checked) return;

                var sourceProjectId = card.dataset.sourceId;
                var targetSelect = card.querySelector('.import-target-select');
                var targetType = targetSelect.value;
                var item = {
                    source_project_id: sourceProjectId,
                    target_type: targetType
                };

                if (targetType === 'create_new') {
                    var nameInput = card.querySelector('.import-name-input');
                    var pathInput = card.querySelector('.import-path-input');
                    item.name = nameInput ? nameInput.value : '';
                    item.repo_folder_path = pathInput ? pathInput.value : '';
                } else {
                    var existingSelect = card.querySelector('.import-existing-select');
                    item.target_project_id = existingSelect ? parseInt(existingSelect.value) : null;
                }
                imports.push(item);
            });

            if (imports.length === 0) {
                alert('No projects enabled for import. Enable at least one project.');
                return;
            }

            var spinner = document.getElementById('import-spinner');
            var btn = document.getElementById('btn-import');
            spinner.style.display = 'inline-block';
            btn.classList.add('is-loading');
            btn.disabled = true;

            apiCall('/api/settings/import/execute', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ raw_json: __importRawJson, imports: imports })
            })
            .then(function(r) {
                if (!r.ok) return r.json().then(function(errData) {
                    throw new Error(errData.error || 'Import failed');
                });
                return r.json();
            })
            .then(function() {
                window.location.href = '/webapp/';
            })
            .catch(function(err) {
                spinner.style.display = 'none';
                btn.classList.remove('is-loading');
                btn.disabled = false;
                var errorDiv = document.getElementById('import-error');
                errorDiv.textContent = 'Import error: ' + err.message;
                errorDiv.style.display = 'block';
            });
        });
    }
});
            "#}
        </script>
    }
}
