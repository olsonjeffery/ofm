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
        <div id="callback-root">
        </div>
        <script>
            {r#"document.addEventListener('DOMContentLoaded',function(){
                var root=document.getElementById('callback-root');
                if(root&&window.__ONBOARDING_HTML__){
                    root.innerHTML=window.__ONBOARDING_HTML__;
                }
            });"#}
        </script>
    }
}
