use leptos::prelude::*;

use crate::webapp::components::settings_sidebar::{
    SettingsSection, SettingsSidebar, SettingsSubPage,
};

/// "Rig-based Providers" sub-page under the "Providers & Agents" settings
/// section. Captures per-vendor Rig provider configs (Anthropic / OpenAI /
/// OpenCode Go / OpenRouter / OpenAI-compatible ± auth) that are persisted as
/// structured JSON files and consumed by Rig clients in a future story (RIG 1).
/// No execution happens on this page.
pub fn render(active: SettingsSubPage) -> String {
    let sidebar = leptos::view! {
        <SettingsSidebar section=SettingsSection::ProvidersAgents active />
    }
    .to_html();

    format!(
        r#"<section class="section">
            <h2 class="title is-3">
                <span class="icon is-medium"><i class="mdi mdi-server-network"></i></span>
                "Rig-based Providers"
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
        sidebar, CONTENT, RIG_PROVIDERS_JS
    )
}

const CONTENT: &str = r#"
<div class="box">
    <p class="mb-3">
        <span class="icon has-text-info"><i class="mdi mdi-information-outline"></i></span>
        Configure providers for Rig-based agents. Each provider is captured as a structured
        JSON config file; execution through Rig clients is coming in a later story.
        Share the provider with your agents via <a href="/webapp/settings/providers-agents/agent-settings">Agent Settings</a>.
    </p>
    <div class="rig-provider-list" id="rig-provider-list">
        <p>Loading...</p>
    </div>
</div>

<div class="box">
    <h3 class="title is-5">
        <span class="icon is-small"><i class="mdi mdi-plus"></i></span>
        Add New Rig Provider
    </h3>
    <div class="field">
        <label class="label" for="rig-name">Name</label>
        <div class="control has-icons-left">
            <input class="input" type="text" id="rig-name" placeholder="e.g. my-anthropic-key"/>
            <span class="icon is-left is-small"><i class="mdi mdi-tag"></i></span>
        </div>
    </div>
    <div class="field">
        <label class="label" for="rig-vendor">Vendor</label>
        <div class="control">
            <div class="select">
                <select id="rig-vendor">
                    <option value="anthropic">Anthropic</option>
                    <option value="open_ai">OpenAI (service)</option>
                    <option value="open_code_go">OpenCode Go</option>
                    <option value="open_router">OpenRouter</option>
                    <option value="open_ai_compatible">OpenAI-compatible (base_url + Bearer)</option>
                    <option value="open_ai_compatible_no_auth">OpenAI-compatible (base_url, no auth)</option>
                </select>
            </div>
        </div>
    </div>
    <div class="field" id="rig-base-url-field">
        <label class="label" for="rig-base-url">Base URL</label>
        <div class="control has-icons-left">
            <input class="input" type="text" id="rig-base-url" placeholder="https://api.example.com/v1"/>
            <span class="icon is-left is-small"><i class="mdi mdi-link-variant"></i></span>
        </div>
    </div>
    <div class="field" id="rig-api-key-field">
        <label class="label" for="rig-api-key">API Key</label>
        <div class="control has-icons-left">
            <input class="input" type="password" id="rig-api-key" placeholder="sk-..."/>
            <span class="icon is-left is-small"><i class="mdi mdi-key"></i></span>
        </div>
    </div>
    <div class="field">
        <label class="label">Model Listing</label>
        <div class="control">
            <div class="select">
                <select id="rig-model-mode">
                    <option value="open_api_list">OpenAPI model-listing endpoint</option>
                    <option value="manual">Manual model list</option>
                </select>
            </div>
            <p class="help" id="rig-model-mode-help">
                OpenAPI mode fetches the model list live from the provider's model-listing API
                when this provider is selected in Agent Settings; manual mode uses the list you
                enter below.
            </p>
        </div>
    </div>
    <div class="field">
        <label class="label" for="rig-models">Models</label>
        <div class="control">
            <textarea class="textarea" id="rig-models" rows="3"
                placeholder="One model id per line, e.g.&#10;gpt-4o"></textarea>
        </div>
        <p class="help">For OpenAPI mode this pre-seeds the cached list; for manual mode it is the source of truth.</p>
    </div>
    <div class="field is-grouped is-grouped-right">
        <div class="control">
            <button class="button is-small is-primary" id="btn-add-rig-provider">
                <span class="icon is-small"><i class="mdi mdi-plus"></i></span>
                <span>Add Provider</span>
            </button>
        </div>
    </div>
</div>

<div class="modal" id="edit-rig-modal">
    <div class="modal-background"></div>
    <div class="modal-card">
        <header class="modal-card-head">
            <p class="modal-card-title">Edit Rig Provider</p>
            <button class="delete" id="btn-close-edit-rig" aria-label="close"></button>
        </header>
        <section class="modal-card-body">
            <input type="hidden" id="edit-rig-id"/>
            <div class="field">
                <label class="label" for="edit-rig-name">Name</label>
                <div class="control">
                    <input class="input" type="text" id="edit-rig-name"/>
                </div>
            </div>
            <div class="field">
                <label class="label" for="edit-rig-vendor">Vendor</label>
                <div class="control">
                    <div class="select">
                        <select id="edit-rig-vendor"></select>
                    </div>
                </div>
            </div>
            <div class="field" id="edit-rig-base-url-field">
                <label class="label" for="edit-rig-base-url">Base URL</label>
                <div class="control">
                    <input class="input" type="text" id="edit-rig-base-url"/>
                </div>
            </div>
            <div class="field" id="edit-rig-api-key-field">
                <label class="label" for="edit-rig-api-key">API Key</label>
                <div class="control">
                    <input class="input" type="password" id="edit-rig-api-key" placeholder="leave blank to keep stored key"/>
                </div>
            </div>
            <div class="field">
                <label class="label" for="edit-rig-model-mode">Model Listing</label>
                <div class="control">
                    <div class="select">
                        <select id="edit-rig-model-mode">
                            <option value="open_api_list">OpenAPI model-listing endpoint</option>
                            <option value="manual">Manual model list</option>
                        </select>
                    </div>
                </div>
            </div>
            <div class="field">
                <label class="label" for="edit-rig-models">Models</label>
                <div class="control">
                    <textarea class="textarea" id="edit-rig-models" rows="4" placeholder="One model id per line"></textarea>
                </div>
            </div>
        </section>
        <footer class="modal-card-foot">
            <div class="field is-grouped is-grouped-right">
                <div class="control">
                    <button class="button is-small is-primary" id="btn-save-edit-rig">Save Changes</button>
                </div>
                <div class="control">
                    <button class="button is-small" id="btn-cancel-edit-rig">Cancel</button>
                </div>
            </div>
        </footer>
    </div>
</div>
"#;

const RIG_PROVIDERS_JS: &str = r#"
window.__ACCESS_TOKEN__ = '';

var RIG_VENDORS = {
    anthropic: { label: 'Anthropic', baseUrl: false, apiKey: true, defaultBaseUrl: null },
    open_ai: { label: 'OpenAI (service)', baseUrl: false, apiKey: true, defaultBaseUrl: null },
    open_code_go: { label: 'OpenCode Go', baseUrl: true, apiKey: true, defaultBaseUrl: 'https://opencode.ai/zen/go/v1' },
    open_router: { label: 'OpenRouter', baseUrl: true, apiKey: true, defaultBaseUrl: null },
    open_ai_compatible: { label: 'OpenAI-compatible (base_url + Bearer)', baseUrl: true, apiKey: true, defaultBaseUrl: null },
    open_ai_compatible_no_auth: { label: 'OpenAI-compatible (base_url, no auth)', baseUrl: true, apiKey: false, defaultBaseUrl: null }
};

function escapeHtml(str) {
    var div = document.createElement('div');
    div.appendChild(document.createTextNode(str == null ? '' : str));
    return div.innerHTML;
}

function applyVendorView(selectId, baseUrlFieldId, apiKeyFieldId, baseUrlInputId, apiKeyInputId) {
    var vendor = document.getElementById(selectId).value;
    var meta = RIG_VENDORS[vendor] || {};
    var baseUrlField = document.getElementById(baseUrlFieldId);
    var apiKeyField = document.getElementById(apiKeyFieldId);
    if (baseUrlField) baseUrlField.style.display = meta.baseUrl ? '' : 'none';
    if (apiKeyField) apiKeyField.style.display = meta.apiKey ? '' : 'none';
    if (meta.baseUrl && meta.defaultBaseUrl) {
        var input = document.getElementById(baseUrlInputId);
        if (input && !input.value.trim()) input.value = meta.defaultBaseUrl;
    }
    if (!meta.apiKey && apiKeyInputId) {
        var keyInput = document.getElementById(apiKeyInputId);
        if (keyInput) keyInput.value = '';
    }
}

function readModels(textareaId) {
    return document.getElementById(textareaId).value
        .split(/[\n,]+/)
        .map(function(s) { return s.trim(); })
        .filter(function(s) { return s.length > 0; });
}

function collectRigForm(prefix, existingApiKey) {
    var mode = document.getElementById(prefix + '-model-mode').value;
    var models = readModels(prefix + '-models');
    var payload = {
        name: document.getElementById(prefix + '-name').value.trim(),
        vendor: document.getElementById(prefix + '-vendor').value,
        base_url: null,
        api_key: null,
        model_list_mode: mode === 'manual' ? { manual: models } : 'open_api_list',
        models: models
    };
    var meta = RIG_VENDORS[payload.vendor] || {};
    if (meta.baseUrl) {
        payload.base_url = document.getElementById(prefix + '-base-url').value.trim();
    }
    if (meta.apiKey) {
        var key = document.getElementById(prefix + '-api-key').value.trim();
        // A blank api key on edit keeps the stored key (the input shows
        // "leave blank to keep stored key").
        payload.api_key = key || existingApiKey || null;
    }
    return payload;
}

function loadRigProviders() {
    var list = document.getElementById('rig-provider-list');
    if (!list) return;
    apiCall('/api/settings/rig-providers')
        .then(function(r) { return r.json(); })
        .then(function(providers) {
            if (providers.length === 0) {
                list.innerHTML = '<p>No Rig providers yet. Add one below.</p>';
                return;
            }
            var html = '<table class="table is-fullwidth is-hoverable"><thead><tr>' +
                '<th>Name</th><th>Vendor</th><th>Model Listing</th><th>Models</th><th>Actions</th>' +
                '</tr></thead><tbody>';
            providers.forEach(function(p) {
                var meta = RIG_VENDORS[p.config.vendor] || { label: p.config.vendor };
                var modeLabel = (p.config.model_list_mode && typeof p.config.model_list_mode === 'object')
                    ? 'Manual (' + (p.config.model_list_mode.manual || []).length + ')'
                    : 'OpenAPI listing';
                var models = (p.config.models || []).join(', ') || '—';
                html += '<tr>';
                html += '<td>' + escapeHtml(p.name) + '</td>';
                html += '<td>' + escapeHtml(meta.label) + '</td>';
                html += '<td>' + escapeHtml(modeLabel) + '</td>';
                html += '<td class="is-size-7">' + escapeHtml(models) + '</td>';
                html += '<td><button class="button is-small" onclick="editRigProvider(\'' + p.id + '\')"><span class="icon is-small"><i class="mdi mdi-pencil"></i></span><span>Edit</span></button> ';
                html += '<button class="button is-small is-danger" onclick="deleteRigProvider(\'' + p.id + '\')"><span class="icon is-small"><i class="mdi mdi-delete"></i></span><span>Delete</span></button></td>';
                html += '</tr>';
            });
            html += '</tbody></table>';
            list.innerHTML = html;
        })
        .catch(function(err) {
            list.innerHTML = '<p class="has-text-danger">Failed to load Rig providers: ' + err + '</p>';
        });
}

window.editRigProvider = function(id) {
    apiCall('/api/settings/rig-providers/' + id)
        .then(function(r) { return r.json(); })
        .then(function(p) {
            var modal = document.getElementById('edit-rig-modal');
            document.getElementById('edit-rig-id').value = p.id;
            document.getElementById('edit-rig-name').value = p.name;
            document.getElementById('edit-rig-models').value = (p.config.models || []).join('\n');
            // Remember the stored key so a blank edit input preserves it.
            window.__EDIT_RIG_API_KEY__ = p.config.api_key || '';
            var vendorSelect = document.getElementById('edit-rig-vendor');
            vendorSelect.innerHTML = '';
            Object.keys(RIG_VENDORS).forEach(function(v) {
                var opt = document.createElement('option');
                opt.value = v;
                opt.textContent = RIG_VENDORS[v].label;
                vendorSelect.appendChild(opt);
            });
            vendorSelect.value = p.config.vendor;
            var modeSelect = document.getElementById('edit-rig-model-mode');
            if (p.config.model_list_mode && typeof p.config.model_list_mode === 'object') {
                modeSelect.value = 'manual';
                var manualModels = (p.config.model_list_mode.manual || []);
                if (manualModels.length) document.getElementById('edit-rig-models').value = manualModels.join('\n');
            } else {
                modeSelect.value = 'open_api_list';
            }
            document.getElementById('edit-rig-base-url').value = p.config.base_url || '';
            document.getElementById('edit-rig-api-key').value = '';
            applyVendorView('edit-rig-vendor', 'edit-rig-base-url-field', 'edit-rig-api-key-field', 'edit-rig-base-url', 'edit-rig-api-key');
            modal.classList.add('is-active');
        })
        .catch(function(err) { alert('Error: ' + err.message); });
};

window.deleteRigProvider = function(id) {
    if (!confirm('Delete this Rig provider?')) return;
    apiCall('/api/settings/rig-providers/' + id, { method: 'DELETE' })
        .then(function(r) {
            if (!r.ok) throw new Error('Delete failed');
            loadRigProviders();
        })
        .catch(function(err) { alert('Error: ' + err.message); });
};

document.addEventListener('DOMContentLoaded', function() {
    loadRigProviders();
    applyVendorView('rig-vendor', 'rig-base-url-field', 'rig-api-key-field', 'rig-base-url', 'rig-api-key');
    document.getElementById('rig-vendor').addEventListener('change', function() {
        applyVendorView('rig-vendor', 'rig-base-url-field', 'rig-api-key-field', 'rig-base-url', 'rig-api-key');
    });

    var btn = document.getElementById('btn-add-rig-provider');
    if (btn) {
        btn.addEventListener('click', function() {
            var payload = collectRigForm('rig');
            if (!payload.name) { alert('Name is required.'); return; }
            apiCall('/api/settings/rig-providers', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify(payload)
            })
            .then(function(r) {
                if (!r.ok) return r.json().then(function(j) { throw new Error(j.error || 'Failed to save'); });
                return r.json();
            })
            .then(function() {
                document.getElementById('rig-name').value = '';
                document.getElementById('rig-models').value = '';
                document.getElementById('rig-api-key').value = '';
                loadRigProviders();
            })
            .catch(function(err) { alert('Error: ' + err.message); });
        });
    }

    var vendorSelect = document.getElementById('edit-rig-vendor');
    if (vendorSelect) {
        vendorSelect.addEventListener('change', function() {
            applyVendorView('edit-rig-vendor', 'edit-rig-base-url-field', 'edit-rig-api-key-field', 'edit-rig-base-url', 'edit-rig-api-key');
        });
    }
    var saveBtn = document.getElementById('btn-save-edit-rig');
    if (saveBtn) {
        saveBtn.addEventListener('click', function() {
            var id = document.getElementById('edit-rig-id').value;
            var payload = collectRigForm('edit-rig', window.__EDIT_RIG_API_KEY__);
            if (!payload.name) { alert('Name is required.'); return; }
            apiCall('/api/settings/rig-providers/' + id, {
                method: 'PUT',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify(payload)
            })
            .then(function(r) {
                if (!r.ok) return r.json().then(function(j) { throw new Error(j.error || 'Update failed'); });
                return r.json();
            })
            .then(function() {
                document.getElementById('edit-rig-modal').classList.remove('is-active');
                loadRigProviders();
            })
            .catch(function(err) { alert('Error: ' + err.message); });
        });
    }
    function closeEditModal() {
        document.getElementById('edit-rig-modal').classList.remove('is-active');
    }
    var cancelBtn = document.getElementById('btn-cancel-edit-rig');
    var closeBtn = document.getElementById('btn-close-edit-rig');
    var modalBg = document.querySelector('#edit-rig-modal .modal-background');
    if (cancelBtn) cancelBtn.addEventListener('click', closeEditModal);
    if (closeBtn) closeBtn.addEventListener('click', closeEditModal);
    if (modalBg) modalBg.addEventListener('click', closeEditModal);
});
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rig_providers_page_renders() {
        let html = render(SettingsSubPage::RigProviders);
        assert!(html.contains("menu"));
        assert!(html.contains("Rig-based Providers"));
        assert!(html.contains("rig-provider-list"));
        assert!(html.contains("rig-vendor"));
        assert!(html.contains("anthropic"));
        assert!(html.contains("open_ai_compatible_no_auth"));
        assert!(html.contains("btn-add-rig-provider"));
        assert!(html.contains("loadRigProviders"));
        assert!(html.contains("__ACCESS_TOKEN__"));
    }

    #[test]
    fn test_rig_providers_page_has_six_vendors() {
        let html = render(SettingsSubPage::RigProviders);
        for vendor in [
            "anthropic",
            "open_ai",
            "open_code_go",
            "open_router",
            "open_ai_compatible",
            "open_ai_compatible_no_auth",
        ] {
            assert!(html.contains(vendor), "expected vendor option for {vendor}");
        }
    }
}
