use leptos::prelude::*;

#[component]
pub fn HomePage() -> impl IntoView {
    view! {
        <main>
            <div class="island-container">
                <div class="island" data-island="uptime">
                    <p>"Loading uptime..."</p>
                </div>
                <script data-island-url="/webapp/islands/uptime"></script>
            </div>
            <div class="island-container">
                <div class="island" data-island="infocard">
                    <p>"Loading info..."</p>
                </div>
                <script data-island-url="/webapp/islands/infocard?title=Welcome&body=Islands+architecture."></script>
            </div>
        </main>
    }
}
