use leptos::prelude::*;

#[component]
pub fn AgentModelSelect() -> impl IntoView {
    view! {
        <div id="agent-model-select">
            <p>"Configure the model and effort for each agent type."</p>
            <table class="agent-model-table" id="agent-model-table">
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
            <button class="btn btn-primary" id="btn-save-agent-models">"Save Agent Settings"</button>
        </div>
    }
}
