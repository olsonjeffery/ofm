use crate::providers::types::ProviderEvent;
use leptos::prelude::*;

fn sanitize_html(html: &str) -> String {
    let allowed_tags: std::collections::HashSet<&str> =
        ["div", "span", "pre", "em", "code", "br", "content"].into();
    let allowed_attrs: std::collections::HashMap<&str, std::collections::HashSet<&str>> = [
        ("div", ["class", "style", "id"].into()),
        ("span", ["class", "style", "id"].into()),
        ("pre", ["class", "style", "id"].into()),
        ("em", ["class", "style", "id"].into()),
        ("code", ["class", "style", "id"].into()),
        ("br", [].into()),
        ("content", ["class", "style", "id"].into()),
    ]
    .into();
    ammonia::Builder::default()
        .tags(allowed_tags)
        .tag_attributes(allowed_attrs)
        .clean(html)
        .to_string()
}

fn render_event(event: &ProviderEvent) -> String {
    match event {
        ProviderEvent::Text { text } => {
            format!(
                r#"<div class="box message-text"><div class="content">{}</div></div>"#,
                text
            )
        }
        ProviderEvent::TextChunk { delta } => {
            format!(r#"<span class="message-chunk">{}</span>"#, delta)
        }
        ProviderEvent::ToolUse {
            tool_name,
            tool_use_id,
            input,
        } => {
            let input_str = serde_json::to_string_pretty(input).unwrap_or_default();
            let id_str = tool_use_id.as_deref().unwrap_or("");
            format!(
                r#"<div class="card"><div class="card-content"><span class="tag is-info is-light">{}</span> <code>{}</code><pre>{}</pre></div></div>"#,
                tool_name, id_str, input_str
            )
        }
        ProviderEvent::ToolResult {
            tool_use_id,
            result,
        } => {
            let id_str = tool_use_id.as_deref().unwrap_or("");
            format!(
                r#"<div class="card"><div class="card-content"><span class="tag is-success is-light">result</span> <code>{}</code><pre>{}</pre></div></div>"#,
                id_str, result
            )
        }
        ProviderEvent::Thinking { thinking } => {
            format!(
                r#"<div class="box message-thinking"><em style="color:#888;">{}</em></div>"#,
                thinking
            )
        }
        ProviderEvent::ThinkingChunk { delta } => {
            format!(
                r#"<span style="color:#888;font-style:italic;">{}</span>"#,
                delta
            )
        }
        ProviderEvent::ContextUsage(usage) => {
            let usage_str = serde_json::to_string(usage).unwrap_or_default();
            format!(
                r#"<div class="notification is-light is-small">{}</div>"#,
                usage_str
            )
        }
        ProviderEvent::Error { error } => {
            format!(
                r#"<div class="notification is-danger is-light">{}</div>"#,
                error
            )
        }
        ProviderEvent::SessionStart { .. } => String::new(),
        ProviderEvent::UserText { text } => {
            format!(
                r#"<content class="content message-user" style="display:block;background:#1565c0;color:#fff;padding:0.75rem;border-radius:6px;white-space:pre-wrap;max-width:33%;margin-left:auto">{}</content>"#,
                text
            )
        }
        ProviderEvent::Ready => String::new(),
        ProviderEvent::ExtensionUiRequest(_) => String::new(),
        ProviderEvent::AvailableCommandsUpdate(_) => String::new(),
        ProviderEvent::Response(data) => {
            let txt = data.as_str().unwrap_or("");
            if txt.is_empty() {
                String::new()
            } else {
                format!(
                    r#"<div class="box message-text"><div class="content">{}</div></div>"#,
                    txt
                )
            }
        }
        ProviderEvent::QuestionAsked {
            question,
            header,
            options,
            ..
        } => {
            let hdr = header.as_deref().unwrap_or("Question");
            let opts_html: String = options
                .iter()
                .map(|o| {
                    format!(
                        r#"<span class="tag is-info is-light">{}</span>"#,
                        o.label
                    )
                })
                .collect::<Vec<_>>()
                .join(" ");
            format!(
                r#"<div class="box"><strong>{}</strong><p>{}</p><div style="margin-top:0.5rem">{}</div></div>"#,
                hdr, question, opts_html
            )
        }
        ProviderEvent::Done(_) => {
            r#"<div class="notification is-success is-light">Done</div>"#.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::types::ProviderEvent;

    #[test]
    fn test_message_stream_empty() {
        let html = leptos::view! { <MessageStream messages=Vec::new() /> }.to_html();
        assert!(html.contains("No messages yet"));
    }

    #[test]
    fn test_message_stream_renders_text() {
        let messages = vec![ProviderEvent::Text {
            text: "Hello World".into(),
        }];
        let html = leptos::view! { <MessageStream messages=messages /> }.to_html();
        assert!(html.contains("Hello World"));
        assert!(html.contains("message-text"));
    }

    #[test]
    fn test_message_stream_renders_tool_use() {
        let messages = vec![ProviderEvent::ToolUse {
            tool_name: "read".into(),
            tool_use_id: Some("id1".into()),
            input: serde_json::json!({"path": "/tmp"}),
        }];
        let html = leptos::view! { <MessageStream messages=messages /> }.to_html();
        assert!(html.contains("read"));
        assert!(html.contains("id1"));
        assert!(html.contains("path"));
    }

    #[test]
    fn test_message_stream_renders_tool_result() {
        let messages = vec![ProviderEvent::ToolResult {
            tool_use_id: Some("id1".into()),
            result: "ok".into(),
        }];
        let html = leptos::view! { <MessageStream messages=messages /> }.to_html();
        assert!(html.contains("result"));
        assert!(html.contains("ok"));
    }

    #[test]
    fn test_message_stream_renders_thinking() {
        let messages = vec![ProviderEvent::Thinking {
            thinking: "hmm".into(),
        }];
        let html = leptos::view! { <MessageStream messages=messages /> }.to_html();
        assert!(html.contains("hmm"));
        assert!(html.contains("message-thinking"));
    }

    #[test]
    fn test_message_stream_renders_error() {
        let messages = vec![ProviderEvent::Error {
            error: "something broke".into(),
        }];
        let html = leptos::view! { <MessageStream messages=messages /> }.to_html();
        assert!(html.contains("something broke"));
        assert!(html.contains("is-danger"));
    }

    #[test]
    fn test_message_stream_renders_done() {
        let messages = vec![ProviderEvent::Done(serde_json::json!({"status": "ok"}))];
        let html = leptos::view! { <MessageStream messages=messages /> }.to_html();
        assert!(html.contains("Done"));
    }
}

#[component]
pub fn MessageStream(messages: Vec<ProviderEvent>) -> impl IntoView {
    let rendered: String = messages
        .iter()
        .map(render_event)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let clean = sanitize_html(&rendered);

    view! {
        <div id="message-stream" class="message-stream" style="flex:1;overflow-y:auto;overflow-x:hidden;padding:1rem;overflow-wrap:break-word">
            {if messages.is_empty() {
                view! { <p class="has-text-grey">"No messages yet. Start a conversation to see messages here."</p> }.into_any()
            } else {
                view! { <div inner_html=clean /> }.into_any()
            }}
        </div>
    }
}
