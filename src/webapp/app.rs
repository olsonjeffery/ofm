use crate::webapp::shim::runtime::global_runtime_script;
use leptos::prelude::*;

#[component]
pub fn ShellPage() -> impl IntoView {
    view! {
        <!DOCTYPE html>
        <html lang="en">
        <head>
            <meta charset="utf-8"/>
            <meta name="viewport" content="width=device-width, initial-scale=1.0"/>
            <title>"omprint"</title>
            <style>{super::styles::bulmaswatch::STYLE_SHEET}</style>
            <style>{super::styles::app::STYLE_SHEET}</style>
            <script>{global_runtime_script()}</script>
        </head>
        <body>
            <header class="section">
                <div class="level">
                    <div class="level-left">
                        <div>
                            <h1 class="title">"omprint"</h1>
                            <p class="subtitle">"AI agent orchestration platform"</p>
                        </div>
                    </div>
                    <nav id="auth-nav" class="level-right" hidden>
                        <a href="/webapp" class="button is-small">"Home"</a>
                        <a href="/webapp/settings" class="button is-small">"Settings"</a>
                        <form action="/api/auth/logout" method="post" id="logout-form">
                            <button type="submit" class="button is-small is-light">"Logout"</button>
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
