use leptos::prelude::*;

#[component]
pub fn ApiKeyManager() -> impl IntoView {
    view! {
        <div id="api-key-manager">
            <p>"Manage your API key for programmatic access."</p>
            <div id="api-key-status">
                <p id="api-key-display" style="display:none">
                    <strong>"Your API Key: "</strong>
                    <code id="api-key-value"></code>
                    <button class="btn" id="btn-copy-key">"Copy"</button>
                </p>
                <p id="api-key-empty">"No API key generated yet."</p>
            </div>
            <div class="api-key-actions">
                <button class="btn btn-primary" id="btn-generate-key">"Generate New API Key"</button>
                <button class="btn" id="btn-revoke-key" style="display:none">"Revoke API Key"</button>
            </div>
        </div>
    }
}
