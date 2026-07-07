use leptos::prelude::*;

#[component]
pub fn LoginPage() -> impl IntoView {
    view! {
        <div class="box has-text-centered">
            <h2 class="title is-4">"Login Required"</h2>
            <p class="subtitle">"You must be logged in to access this content."</p>
            <a href="/webapp/login" class="button is-primary">"Login"</a>
        </div>
    }
}

pub fn render_login_required() -> String {
    crate::webapp::shim::render_component(|| view! { <LoginPage /> })
}
