use crate::webapp::shim::runtime::global_runtime_script;
use leptos::prelude::*;
use leptos_styling::style_sheet;

style_sheet!(app_styles, "src/webapp/styles/app.css", "app_styles");

#[component]
pub fn ShellPage() -> impl IntoView {
    view! {
        <!DOCTYPE html>
        <html lang="en">
        <head>
            <meta charset="utf-8"/>
            <meta name="viewport" content="width=device-width, initial-scale=1.0"/>
            <title>"omprint"</title>
            <style>{STYLE_SHEET}</style>
            <script>{global_runtime_script()}</script>
        </head>
        <body>
            <header class="page-header">
                <div class="page-header-row">
                    <div>
                        <h1>"omprint"</h1>
                        <p>"AI agent orchestration platform"</p>
                    </div>
                    <nav id="auth-nav" class="auth-nav" hidden>
                        <form action="/api/auth/logout" method="post" id="logout-form">
                            <button type="submit" class="btn btn-logout">"Logout"</button>
                        </form>
                    </nav>
                </div>
            </header>
            <main></main>
            <script>
                {r#"document.addEventListener('DOMContentLoaded',function(){
                    var form=document.getElementById('logout-form');
                    if(!form)return;
                    form.addEventListener('submit',function(ev){
                        ev.preventDefault();
                        fetch(form.action,{method:'POST',credentials:'same-origin'})
                            .then(function(){window.location.href='/webapp/login';})
                            .catch(function(){window.location.href='/webapp/login';});
                    });
                });"#}
            </script>
        </body>
        </html>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shell_page_contains_html_and_script() {
        let html = leptos::view! { <ShellPage /> }.to_html();
        assert!(html.contains("<html"));
        assert!(html.contains("data-island-url"));
        assert!(html.contains("omprint"));
    }
}
