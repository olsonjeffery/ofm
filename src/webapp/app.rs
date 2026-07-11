use crate::webapp::components::navbar::Navbar;
use crate::webapp::shim::runtime::global_runtime_script;
use leptos::prelude::*;

#[component]
pub fn ShellPage(user_json: Option<String>) -> impl IntoView {
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
            <style>{super::styles::app::STYLE_SHEET}</style>
            <script>{global_runtime_script()}</script>
        </head>
        <body>
            <Navbar user_json />
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
        let html = leptos::view! { <ShellPage user_json /> }.to_html();
        assert!(html.contains("<html"));
        assert!(html.contains("data-island-url"));
        assert!(html.contains("ofm"));
        assert!(html.contains("navbar"));
        assert!(html.contains("materialdesignicons"));
    }
}
