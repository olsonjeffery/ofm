use leptos::prelude::*;

use crate::webapp::components::settings_sidebar::{
    SettingsSection, SettingsSidebar, SettingsSubPage,
};

pub fn render(active: SettingsSubPage) -> String {
    let sidebar = leptos::view! {
        <SettingsSidebar section=SettingsSection::ImportExport active />
    }
    .to_html();

    let (pane, js) = match active {
        SettingsSubPage::Export => (
            leptos::view! {
                <div class="box">
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
            }
            .to_html(),
            EXPORT_JS,
        ),
        SettingsSubPage::Import => (
            leptos::view! {
                <div class="box">
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
            }
            .to_html(),
            IMPORT_JS,
        ),
        _ => unreachable!("import-export page only renders its own sub-pages"),
    };

    format!(
        r#"<section class="section">
            <h2 class="title is-3">
                <span class="icon is-medium"><i class="mdi mdi-export-variant"></i></span>
                "Import/Export"
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

const EXPORT_JS: &str = r#"
window.__ACCESS_TOKEN__ = '';

document.addEventListener('DOMContentLoaded', function() {
    loadExportTab();
});

function escapeHtml(str) {
    var div = document.createElement('div');
    div.appendChild(document.createTextNode(str));
    return div.innerHTML;
}

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
"#;

const IMPORT_JS: &str = r#"
window.__ACCESS_TOKEN__ = '';

function escapeHtml(str) {
    var div = document.createElement('div');
    div.appendChild(document.createTextNode(str));
    return div.innerHTML;
}

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
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_import_export_renders_export_landing() {
        let html = render(SettingsSubPage::Export);
        assert!(html.contains("menu"));
        assert!(html.contains("Export Projects"));
        assert!(html.contains("export-project-list"));
        assert!(html.contains("__ACCESS_TOKEN__"));
        assert!(html.contains("loadExportTab"));
        assert!(html.contains("btn-export"));
        assert!(!html.contains("import-file-input"));
    }

    #[test]
    fn test_import_export_renders_import() {
        let html = render(SettingsSubPage::Import);
        assert!(html.contains("menu"));
        assert!(html.contains("Import Projects"));
        assert!(html.contains("import-file-input"));
        assert!(html.contains("btn-import"));
        assert!(html.contains("__importRawJson"));
        assert!(!html.contains("export-project-list"));
    }
}
