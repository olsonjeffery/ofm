use leptos::prelude::*;

#[component]
pub fn ConfigBodyEditor() -> impl IntoView {
    view! {
        <div id="config-body-editor">
            <p>"Manage your model configuration entries below. Each entry stores a named JSON configuration body."</p>
            <div class="config-list" id="config-list">
                <p>"Loading..."</p>
            </div>
            <div class="box">
                <h3 class="title is-5">"Add New Configuration"</h3>
                <div class="field">
                    <label class="label" for="new-config-name">"Name"</label>
                    <div class="control has-icons-left">
                        <input class="input" type="text" id="new-config-name" placeholder="e.g. my-model-config"/>
                        <span class="icon is-left is-small"><i class="mdi mdi-tag"></i></span>
                    </div>
                </div>
                <div class="field">
                    <label class="label" for="new-config-harness">"Harness"</label>
                    <div class="control">
                        <div class="select">
                            <select id="new-config-harness">
                                <option value="opencode">opencode</option>
                            </select>
                        </div>
                    </div>
                </div>
                <div class="field">
                    <label class="label" for="new-config-body">"Config Body (JSON)"</label>
                    <div class="control">
                        <textarea class="textarea" id="new-config-body" rows="8" placeholder="Paste JSON configuration here..."></textarea>
                    </div>
                </div>
                <button class="button is-primary" id="btn-add-config">
                    <span class="icon is-small"><i class="mdi mdi-plus"></i></span>
                    <span>"Add Configuration"</span>
                </button>
            </div>
        </div>

        <div class="modal" id="edit-config-modal">
            <div class="modal-background"></div>
            <div class="modal-card">
                <header class="modal-card-head">
                    <p class="modal-card-title">"Edit Configuration"</p>
                    <button class="delete" id="btn-close-edit-modal" aria-label="close"></button>
                </header>
                <section class="modal-card-body">
                    <input type="hidden" id="edit-config-id"/>
                    <div class="field">
                        <label class="label" for="edit-config-name">"Name"</label>
                        <div class="control">
                            <input class="input" type="text" id="edit-config-name" placeholder="e.g. my-model-config"/>
                        </div>
                    </div>
                    <div class="field">
                        <label class="label" for="edit-config-harness">"Harness"</label>
                        <div class="control">
                            <div class="select">
                                <select id="edit-config-harness">
                                    <option value="opencode">opencode</option>
                                </select>
                            </div>
                        </div>
                    </div>
                    <div class="field">
                        <label class="label" for="edit-config-body">"Config Body (JSON)"</label>
                        <div class="control">
                            <textarea class="textarea" id="edit-config-body" rows="8" placeholder="Paste JSON configuration here..."></textarea>
                        </div>
                    </div>
                </section>
                <footer class="modal-card-foot">
                    <button class="button is-primary" id="btn-save-edit-config">"Save Changes"</button>
                    <button class="button" id="btn-cancel-edit-config">"Cancel"</button>
                </footer>
            </div>
        </div>
    }
}
