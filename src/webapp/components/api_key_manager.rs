use leptos::prelude::*;

#[component]
pub fn ApiKeyManager() -> impl IntoView {
    view! {
        <div id="api-key-manager">
            <p>"Manage your API key for programmatic access."</p>
            <div id="api-key-status">
                <p id="api-key-display" hidden>
                    <strong>"Your API Key: "</strong>
                    <code id="api-key-value"></code>
                    <button class="button is-small" id="btn-copy-key">"Copy"</button>
                </p>
                <p id="api-key-empty">"No API key generated yet."</p>
            </div>
            <div class="field is-grouped">
                <button class="button is-primary" id="btn-generate-key">"Generate New API Key"</button>
                <button class="button is-small" id="btn-revoke-key" hidden>"Revoke API Key"</button>
            </div>
        </div>
    }
}
