use leptos::prelude::*;

#[component]
pub fn MarkdownViewer(content: String) -> impl IntoView {
    let parser = pulldown_cmark::Parser::new(&content);
    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, parser);
    let clean = ammonia::Builder::default()
        .add_tags(&[
            "h1",
            "h2",
            "h3",
            "h4",
            "h5",
            "h6",
            "p",
            "br",
            "hr",
            "ul",
            "ol",
            "li",
            "blockquote",
            "pre",
            "code",
            "table",
            "thead",
            "tbody",
            "tr",
            "th",
            "td",
            "a",
            "strong",
            "em",
            "del",
            "ins",
            "sub",
            "sup",
            "img",
        ])
        .add_tag_attributes("a", &["href"])
        .add_tag_attributes("img", &["src", "alt", "title"])
        .clean(&html)
        .to_string();
    view! { <div class="content" inner_html=clean></div> }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_basic_markdown() {
        let md = "# Hello\n\nThis is **bold** and *italic*.\n\n- item 1\n- item 2\n\n```\ncode block\n```\n\n[link](http://example.com)"
            .to_string();
        let html = leptos::view! { <MarkdownViewer content=md /> }.to_html();
        assert!(html.contains("<h1>"));
        assert!(html.contains("Hello"));
        assert!(html.contains("<strong>"));
        assert!(html.contains("bold"));
        assert!(html.contains("<em>"));
        assert!(html.contains("italic"));
        assert!(html.contains("<ul>"));
        assert!(html.contains("<li>item 1"));
        assert!(html.contains("<code>"));
        assert!(html.contains("code block"));
        assert!(html.contains("<a href"));
        assert!(html.contains("class=\"content\""));
    }

    #[test]
    fn test_render_empty_content() {
        let content = String::new();
        let html = leptos::view! { <MarkdownViewer content=content /> }.to_html();
        assert!(html.contains("class=\"content\""));
        assert!(!html.contains("<h1>"));
        assert!(!html.contains("<p>"));
    }
}
