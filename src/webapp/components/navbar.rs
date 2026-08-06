use crate::db::schema::ActiveAgent;
use crate::webapp::components::breadcrumb::BreadcrumbItem;
use crate::webapp::components::breadcrumb::Breadcrumbs;
use leptos::prelude::*;

#[component]
pub fn Navbar(
    user_json: Option<String>,
    breadcrumbs: Vec<BreadcrumbItem>,
    active_agents: Vec<ActiveAgent>,
) -> impl IntoView {
    let is_logged_in = user_json.is_some();
    let parsed_user = user_json
        .as_ref()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok());
    let username = parsed_user
        .as_ref()
        .and_then(|v| {
            v.get("username")
                .and_then(|u| u.as_str().map(|s| s.to_string()))
        })
        .unwrap_or_default();
    let is_admin = parsed_user
        .as_ref()
        .and_then(|v| v.get("is_admin").and_then(|a| a.as_bool()))
        .unwrap_or(false);

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
                    <crate::webapp::components::agent_dropdown::AgentDropdown active_agents />
                    <Breadcrumbs breadcrumbs />
                </div>
                <div class="navbar-end">
                    {if is_logged_in {
                        view! {
                            <span class="navbar-item">
                                <span class="icon is-small"><i class="mdi mdi-account"></i></span>
                                <span>{username}</span>
                            </span>
                            {if is_admin {
                                view! {
                                    <span class="navbar-item">
                                        <a class="button is-white is-small" href="/webapp/settings/admin/groups">
                                            <span class="icon is-small"><i class="mdi mdi-account-group"></i></span>
                                            <span>"Groups"</span>
                                        </a>
                                    </span>
                                }
                                    .into_any()
                            } else {
                                ().into_any()
                            }}
                            <crate::webapp::components::settings_dropdown::SettingsDropdown />
                            <div class="navbar-item">
                                <form action="/api/auth/logout" method="post" id="logout-form">
                                    <button type="submit" class="button is-primary is-light is-small">
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
        let breadcrumbs = Vec::new();
        let html =
            leptos::view! { <Navbar user_json breadcrumbs active_agents=Vec::new() /> }.to_html();
        assert!(html.contains("Login"));
        assert!(html.contains("/webapp/login"));
        assert!(html.contains("mdi-login"));
        assert!(html.contains("ofm"));
    }

    #[test]
    fn test_navbar_renders_user_info_when_logged_in() {
        let user = serde_json::json!({ "username": "test@example.com" });
        let user_json = Some(user.to_string());
        let breadcrumbs = Vec::new();
        let html =
            leptos::view! { <Navbar user_json breadcrumbs active_agents=Vec::new() /> }.to_html();
        assert!(html.contains("test@example.com"));
        assert!(html.contains("Logout"));
        assert!(html.contains("Settings"));
        assert!(html.contains("mdi-logout"));
        assert!(html.contains("mdi-cog"));
        assert!(html.contains("mdi-account"));
        assert!(html.contains("mdi-arrow-down-bold"));
        assert!(html.contains("/webapp/settings"));
        assert!(html.contains("/webapp/settings/providers-agents"));
        assert!(html.contains("/webapp/settings/import-export"));
        assert!(html.contains("/webapp/settings/account"));
        assert!(html.contains("settings-dropdown-trigger"));
        assert!(!html.contains("User Config"));
    }

    #[test]
    fn test_navbar_hides_groups_entry_for_non_admin() {
        let user = serde_json::json!({ "username": "regular@example.com", "is_admin": false });
        let user_json = Some(user.to_string());
        let html =
            leptos::view! { <Navbar user_json breadcrumbs=Vec::new() active_agents=Vec::new() /> }
                .to_html();
        assert!(html.contains("regular@example.com"));
        assert!(
            !html.contains("/webapp/settings/admin/groups"),
            "non-admin must not see Groups"
        );
    }

    #[test]
    fn test_navbar_shows_groups_entry_for_admin() {
        let user = serde_json::json!({ "username": "admin@localhost", "is_admin": true });
        let user_json = Some(user.to_string());
        let html =
            leptos::view! { <Navbar user_json breadcrumbs=Vec::new() active_agents=Vec::new() /> }
                .to_html();
        assert!(html.contains("/webapp/settings/admin/groups"));
        assert!(html.contains("mdi-account-group"));
    }

    #[test]
    fn test_navbar_contains_logo_link() {
        let user_json: Option<String> = None;
        let breadcrumbs = Vec::new();
        let html =
            leptos::view! { <Navbar user_json breadcrumbs active_agents=Vec::new() /> }.to_html();
        assert!(html.contains("/webapp"));
        assert!(html.contains("ofm-logo-white-no-bg.png"));
    }

    #[test]
    fn test_navbar_renders_breadcrumbs() {
        let user_json: Option<String> = None;
        let breadcrumbs =
            vec![crate::webapp::components::breadcrumb::breadcrumb_registry::all_projects()];
        let html =
            leptos::view! { <Navbar user_json breadcrumbs active_agents=Vec::new() /> }.to_html();
        assert!(html.contains("All Projects"));
        assert!(html.contains("mdi-home"));
        assert!(html.contains("breadcrumb"));
    }

    #[test]
    fn test_navbar_renders_agent_dropdown() {
        let user_json: Option<String> = None;
        let breadcrumbs = Vec::new();
        let html =
            leptos::view! { <Navbar user_json breadcrumbs active_agents=Vec::new() /> }.to_html();
        assert!(html.contains("mdi-message-outline"));
        assert!(html.contains("agent-dropdown"));
        assert!(html.contains("ws-status-entry"));
    }
}
