use crate::webapp::components::navbar::Navbar;
use crate::webapp::shim::runtime::global_runtime_script;
use leptos::prelude::*;

#[component]
pub fn ShellPage(user_json: Option<String>) -> impl IntoView {
    view! {
        <!DOCTYPE html>
        <html lang="en">
        <head>
            <meta charset="utf-8"/>
            <meta name="viewport" content="width=device-width, initial-scale=1.0"/>
            <title>"omprint"</title>
            <link rel="stylesheet" href="/webapp/assets/bulma.css" />
            <link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/@mdi/font@7.4.47/css/materialdesignicons.min.css" />
            <style>{super::styles::app::STYLE_SHEET}</style>
            <script>{global_runtime_script()}</script>
        </head>
        <body>
            <Navbar user_json />
            <main style="width: 95%; margin: 0 auto; min-height: calc(100vh - 3.25rem);"></main>
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
        let html = leptos::view! { <ShellPage user_json /> }.to_html();
        assert!(html.contains("<html"));
        assert!(html.contains("data-island-url"));
        assert!(html.contains("omprint"));
        assert!(html.contains("navbar"));
        assert!(html.contains("materialdesignicons"));
    }
}
