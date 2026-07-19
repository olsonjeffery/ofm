use crate::webapp::components::breadcrumb::BreadcrumbItem;
use crate::webapp::components::navbar::Navbar;
use crate::webapp::shim::runtime::global_runtime_script;
use leptos::prelude::*;

#[component]
pub fn ShellPage(user_json: Option<String>, breadcrumbs: Vec<BreadcrumbItem>) -> impl IntoView {
    view! {
        <!DOCTYPE html>
        <html lang="en">
        <head>
            <link rel="icon" type="image/svg+xml" href="/webapp/assets/ofm-logo.svg" />
            <link rel="icon" type="image/png" href="/webapp/assets/ofm-logo.png" />
            <meta charset="utf-8"/>
            <meta name="viewport" content="width=device-width, initial-scale=1.0"/>
            <title>"ofm"</title>
            <link rel="stylesheet" href="/webapp/assets/bulma.css" />
            <link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/@mdi/font@7.4.47/css/materialdesignicons.min.css" />
            <style>{include_str!("styles/app.css")}</style>
            <script>{global_runtime_script()}</script>
        </head>
        <body>
            <Navbar user_json breadcrumbs />
            <main></main>
        </body>
        </html>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shell_page_contains_html_and_script() {
        let user_json: Option<String> = None;
        let breadcrumbs = Vec::new();
        let html = leptos::view! { <ShellPage user_json breadcrumbs /> }.to_html();
        assert!(html.contains("<html"));
        assert!(html.contains("data-island-url"));
        assert!(html.contains("ofm"));
        assert!(html.contains("navbar"));
        assert!(html.contains("materialdesignicons"));
    }

    #[test]
    fn test_shell_page_main_tag_exact_match() {
        let user_json: Option<String> = None;
        let breadcrumbs = Vec::new();
        let html = leptos::view! { <ShellPage user_json breadcrumbs /> }.to_html();
        let search = "<main></main>";
        assert!(
            html.contains(search),
            "Shell HTML does not contain exact main tag match. Search: {}",
            search
        );
    }
}
