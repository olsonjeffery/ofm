use leptos::prelude::*;

#[component]
pub fn LoginPage() -> impl IntoView {
    view! {
        <div class="auth-failure">
            <h2>"Login Required"</h2>
            <p>"You must be logged in to access this content."</p>
            <a href="/webapp/login" class="btn">"Login"</a>
        </div>
    }
}

pub fn render_login_required() -> String {
    crate::webapp::shim::render_component(|| view! { <LoginPage /> })
}
