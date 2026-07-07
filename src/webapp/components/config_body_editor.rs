use leptos::prelude::*;

#[component]
pub fn ConfigBodyEditor() -> impl IntoView {
    view! {
        <div id="config-body-editor">
            <p>"Manage your model configuration entries below. Each entry stores a named YAML or JSON configuration body."</p>
            <div class="config-list" id="config-list">
                <p>"Loading..."</p>
            </div>
            <div class="box">
                <h3 class="title is-5">"Add New Configuration"</h3>
                <div class="field">
                    <label class="label" for="new-config-name">"Name"</label>
                    <div class="control">
                        <input class="input" type="text" id="new-config-name" placeholder="e.g. my-model-config"/>
                    </div>
                </div>
                <div class="field">
                    <label class="label" for="new-config-harness">"Harness"</label>
                    <div class="control">
                        <input class="input" type="text" id="new-config-harness" placeholder="e.g. openai"/>
                    </div>
                </div>
                <div class="field">
                    <label class="label" for="new-config-body">"Config Body (YAML or JSON)"</label>
                    <div class="control">
                        <textarea class="textarea" id="new-config-body" rows="8" placeholder="Paste YAML or JSON configuration here..."></textarea>
                    </div>
                </div>
                <button class="button is-primary" id="btn-add-config">"Add Configuration"</button>
            </div>
        </div>
    }
}
