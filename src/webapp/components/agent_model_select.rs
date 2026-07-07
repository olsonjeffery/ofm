use leptos::prelude::*;

#[component]
pub fn AgentModelSelect() -> impl IntoView {
    view! {
        <div id="agent-model-select">
            <p>"Configure the model and effort for each agent type."</p>
            <table class="table is-fullwidth is-hoverable" id="agent-model-table">
                <thead>
                    <tr>
                        <th>"Agent Type"</th>
                        <th>"Model"</th>
                        <th>"Effort"</th>
                    </tr>
                </thead>
                <tbody id="agent-model-tbody">
                    <tr>
                        <td colspan="3">"Loading..."</td>
                    </tr>
                </tbody>
            </table>
            <button class="button is-primary" id="btn-save-agent-models">
                <span class="icon is-small"><i class="mdi mdi-content-save"></i></span>
                <span>"Save Agent Settings"</span>
            </button>
        </div>
    }
}
