use leptos::prelude::*;

#[component]
pub fn CallbackPage(
    access_token: String,
    user_json: String,
    onboarding_html: String,
) -> impl IntoView {
    view! {
        <script>
            {format!(
                "window.__ACCESS_TOKEN__ = '{}'; window.__USER__ = {}; window.__ONBOARDING_HTML__ = {};",
                access_token,
                user_json.replace("</", "<\\/"),
                onboarding_html.replace("</", "<\\/"),
            )}
        </script>
        <div id="callback-root" class="callback-loading">
            <p>"Completing sign-in..."</p>
        </div>
    }
}
