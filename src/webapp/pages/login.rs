use leptos::prelude::*;

#[component]
pub fn LoginPage() -> impl IntoView {
    view! {
        <div class="login-page">
            <div class="login-card">
                <h2>"Sign in to omprint"</h2>
                <p>"Authenticate with your SSO provider to continue."</p>
                <button id="sso-login-btn" class="btn btn-primary">
                    "Sign in with SSO"
                </button>
            </div>
        </div>
    }
}

pub fn render_login_page() -> String {
    crate::webapp::shim::render_component(|| view! { <LoginPage /> })
}
