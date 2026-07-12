use leptos::prelude::*;

#[component]
pub fn Navbar(user_json: Option<String>) -> impl IntoView {
    let is_logged_in = user_json.is_some();
    let username = user_json
        .as_ref()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
        .and_then(|v| {
            v.get("username")
                .and_then(|u| u.as_str().map(|s| s.to_string()))
        })
        .unwrap_or_default();

    view! {
        <nav class="navbar is-fixed-top" role="navigation" aria-label="main navigation">
            <div class="navbar-brand">
                <a class="navbar-item" href="/webapp">
                    <img src="/webapp/assets/ofm-logo-white-no-bg.png" class="header-logo" />
                    <strong style="color: var(--bulma-white);writing-mode: tb-rl; margin:none; padding:2px; border-right: solid 1px var(--bulma-white)">"ofm"</strong>
                </a>
            </div>
            <div class="navbar-menu">
                <div class="navbar-start">
                    <crate::webapp::components::ws_status::WsStatus />
                </div>
                <div class="navbar-end">
                    {if is_logged_in {
                        view! {
                            <span class="navbar-item">
                                <span class="icon is-small"><i class="mdi mdi-account"></i></span>
                                <span>{username}</span>
                            </span>
                            <div class="navbar-item">
                                <a href="/webapp/onboarding" class="button is-white">
                                    <span class="icon is-small"><i class="mdi mdi-account-cog"></i></span>
                                    <span>"User Config"</span>
                                </a>
                            </div>
                            <div class="navbar-item">
                                <a href="/webapp/settings" class="button is-white">
                                    <span class="icon is-small"><i class="mdi mdi-cog"></i></span>
                                    <span>"Settings"</span>
                                </a>
                            </div>
                            <div class="navbar-item">
                                <form action="/api/auth/logout" method="post" id="logout-form">
                                    <button type="submit" class="button is-white">
                                        <span class="icon is-small"><i class="mdi mdi-logout"></i></span>
                                        <span>"Logout"</span>
                                    </button>
                                </form>
                            </div>
                        }
                            .into_any()
                    } else {
                        view! {
                            <div class="navbar-item">
                                <crate::webapp::islands::sso_login::SsoLoginButton label="Login" />
                            </div>
                        }
                            .into_any()
                    }}
                </div>
            </div>
        </nav>
        <script>
            {r#"document.addEventListener('DOMContentLoaded',function(){
                var form=document.getElementById('logout-form');
                if(!form)return;
                form.addEventListener('submit',function(ev){
                    ev.preventDefault();
                    fetch(form.action,{method:'POST',credentials:'same-origin'})
                        .then(function(r){return r.json();})
                        .then(function(d){window.location.href=d.redirect_url||'/webapp/login';})
                        .catch(function(){window.location.href='/webapp/login';});
                });
            });"#}
        </script>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_navbar_renders_login_button_when_anonymous() {
        let user_json: Option<String> = None;
        let html = leptos::view! { <Navbar user_json /> }.to_html();
        assert!(html.contains("Login"));
        assert!(html.contains("/webapp/login"));
        assert!(html.contains("mdi-login"));
        assert!(html.contains("ofm"));
    }

    #[test]
    fn test_navbar_renders_user_info_when_logged_in() {
        let user = serde_json::json!({ "username": "test@example.com" });
        let user_json = Some(user.to_string());
        let html = leptos::view! { <Navbar user_json /> }.to_html();
        assert!(html.contains("test@example.com"));
        assert!(html.contains("Logout"));
        assert!(html.contains("Settings"));
        assert!(html.contains("mdi-logout"));
        assert!(html.contains("mdi-cog"));
        assert!(html.contains("mdi-account"));
    }

    #[test]
    fn test_navbar_contains_logo_link() {
        let user_json: Option<String> = None;
        let html = leptos::view! { <Navbar user_json /> }.to_html();
        assert!(html.contains("/webapp"));
        assert!(html.contains("ofm-logo-white-no-bg.png"));
    }
}
