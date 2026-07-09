use leptos::prelude::*;

#[component]
pub fn LoginPage() -> impl IntoView {
    view! {
        <section class="section">
            <div class="columns is-centered">
                <div class="column is-half">
                    <div class="box has-text-centered">
                        <h2>"Sign in to ofm"</h2>
                        <p>"Authenticate with your SSO provider to continue."</p>
                        <crate::webapp::islands::sso_login::SsoLoginButton label="Sign in with SSO" />
                    </div>
                </div>
            </div>
        </section>
    }
}

pub fn render_login_page() -> String {
    crate::webapp::shim::render_component(|| view! { <LoginPage /> })
}
