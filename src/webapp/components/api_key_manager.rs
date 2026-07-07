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
                    <button class="button is-small" id="btn-copy-key">
                        <span class="icon is-small"><i class="mdi mdi-content-copy"></i></span>
                        <span>"Copy"</span>
                    </button>
                </p>
                <p id="api-key-empty">"No API key generated yet."</p>
            </div>
            <div class="field is-grouped">
                <button class="button is-primary" id="btn-generate-key">
                    <span class="icon is-small"><i class="mdi mdi-key-plus"></i></span>
                    <span>"Generate New API Key"</span>
                </button>
                <button class="button is-small" id="btn-revoke-key" hidden>
                    <span class="icon is-small"><i class="mdi mdi-key-remove"></i></span>
                    <span>"Revoke API Key"</span>
                </button>
            </div>
        </div>
    }
}
