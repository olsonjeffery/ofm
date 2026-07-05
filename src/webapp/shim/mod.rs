pub mod runtime;

use leptos::prelude::*;

pub fn render_component<F, V>(f: F) -> String
where
    F: FnOnce() -> V + 'static,
    V: IntoView,
{
    f().to_html()
}

pub fn wrap_island(name: &str, path: &str, query_string: &str, html: String) -> String {
    let url = if query_string.is_empty() {
        path.to_string()
    } else {
        format!("{}?{}", path, query_string)
    };
    format!(
        r#"<div class="island" data-island="{name}">{html}</div>
<script data-island-url="{url}">/* island fetch handled by global runtime */</script>"#,
        name = name,
        html = html,
        url = url,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_component_returns_non_empty_string() {
        let result = render_component(|| view! { <p>"hello"</p> });
        assert!(!result.is_empty());
        assert!(result.contains("hello"));
    }

    #[test]
    fn test_wrap_island_produces_correct_html() {
        let html = wrap_island("test-island", "/webapp/islands/test", "", "<p>content</p>".into());
        assert!(html.contains(r#"data-island="test-island""#));
        assert!(html.contains(r#"data-island-url="/webapp/islands/test""#));
        assert!(html.contains("<p>content</p>"));
    }

    #[test]
    fn test_wrap_island_with_query_string() {
        let html = wrap_island("test", "/webapp/islands/test", "key=value", "<p>content</p>".into());
        assert!(html.contains(r#"data-island-url="/webapp/islands/test?key=value""#));
    }
}
