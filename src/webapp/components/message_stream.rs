use crate::providers::types::ProviderEvent;
use leptos::prelude::*;
use std::sync::atomic::{AtomicU64, Ordering};

static COLLAPSE_ID: AtomicU64 = AtomicU64::new(0);

fn next_id() -> String {
    format!("c{}", COLLAPSE_ID.fetch_add(1, Ordering::Relaxed))
}

fn esc(s: &str) -> String {
    html_escape::encode_text(s).to_string()
}

fn maybe_collapse(content: &str, html_id: &str) -> String {
    if content.len() <= 400 {
        esc(content)
    } else {
        format!(
            r##"<span id="preview-{}">{}</span><a href="#" id="btn-{}" class="show-more-btn" onclick="toggleShowMore('{}');return false">show more</a><span id="full-{}" style="display:none">{}</span>"##,
            esc(html_id),
            esc(&content[..400]),
            esc(html_id),
            esc(html_id),
            esc(html_id),
            esc(content)
        )
    }
}

fn render_event(event: &ProviderEvent) -> String {
    match event {
        ProviderEvent::Text { text } => {
            let id = next_id();
            let content = maybe_collapse(text, &id);
            format!(
                r#"<div class="message-model"><div class="content">{}</div></div>"#,
                content
            )
        }
        ProviderEvent::TextChunk { .. } => String::new(),
        ProviderEvent::ToolUse {
            tool_name,
            tool_use_id,
            input,
        } => {
            let input_str = serde_json::to_string_pretty(input).unwrap_or_default();
            let id_str = tool_use_id.as_deref().unwrap_or("");
            let collapse_id = if id_str.is_empty() {
                next_id()
            } else {
                id_str.to_string()
            };
            let content = maybe_collapse(&input_str, &collapse_id);
            let data_attr = if !id_str.is_empty() {
                format!(r#" data-tool-use-id="{}""#, esc(id_str))
            } else {
                String::new()
            };
            format!(
                r#"<div class="message-tool"{}><span class="icon"><i class="mdi mdi-cog-outline"></i></span> <code>{}</code><div class="tool-content" style="white-space:pre-wrap;word-break:break-word;overflow-wrap:break-word;max-width:100%">{}</div></div>"#,
                data_attr,
                esc(tool_name),
                content
            )
        }
        ProviderEvent::ToolResult {
            tool_use_id,
            result,
        } => {
            let id_str = tool_use_id.as_deref().unwrap_or("");
            let collapse_id = if id_str.is_empty() {
                next_id()
            } else {
                id_str.to_string()
            };
            let content = maybe_collapse(result, &collapse_id);
            let data_attr = if !id_str.is_empty() {
                format!(r#" data-tool-use-id="{}""#, esc(id_str))
            } else {
                String::new()
            };
            format!(
                r#"<div class="message-tool"{}><span class="icon"><i class="mdi mdi-cog-outline"></i></span><div class="tool-content" style="white-space:pre-wrap;word-break:break-word;overflow-wrap:break-word;max-width:100%">{}</div></div>"#,
                data_attr, content
            )
        }
        ProviderEvent::Thinking { thinking } => {
            let id = next_id();
            let content = maybe_collapse(thinking, &id);
            format!(
                r#"<div class="message-thinking"><span class="icon"><i class="mdi mdi-snowflake-outline"></i></span>{}</div>"#,
                content
            )
        }
        ProviderEvent::ThinkingChunk { .. } => String::new(),
        ProviderEvent::ContextUsage(usage) => {
            let usage_str = serde_json::to_string(usage).unwrap_or_default();
            format!(
                r#"<div class="notification is-light is-small">{}</div>"#,
                esc(&usage_str)
            )
        }
        ProviderEvent::Error { error } => {
            format!(
                r#"<div class="notification is-danger is-light">{}</div>"#,
                esc(error)
            )
        }
        ProviderEvent::SessionStart { .. } => String::new(),
        ProviderEvent::UserText { text } => {
            format!(
                r#"<content class="content message-user">{}</content>"#,
                esc(text)
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
                let id = next_id();
                let content = maybe_collapse(txt, &id);
                format!(
                    r#"<div class="message-model"><div class="content">{}</div></div>"#,
                    content
                )
            }
        }
        ProviderEvent::QuestionAsked { ref questions, .. } => {
            let mut html = String::new();
            for q in questions {
                let hdr = q.header.as_deref().unwrap_or("Question");
                let opts_html: String = q
                    .options
                    .iter()
                    .map(|o| {
                        format!(
                            r#"<span class="tag is-info is-light">{}</span>"#,
                            esc(&o.label)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                html.push_str(&format!(
                    r#"<div class="box"><strong>{}</strong><p>{}</p><div style="margin-top:0.5rem">{}</div></div>"#,
                    esc(hdr), esc(&q.question), opts_html
                ));
            }
            html
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
        assert!(html.contains("message-model"));
        assert!(!html.contains("message-text"));
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
        assert!(html.contains("message-tool"));
        assert!(html.contains("mdi-cog-outline"));
        assert!(html.contains(r#"data-tool-use-id="id1""#));
    }

    #[test]
    fn test_message_stream_renders_tool_result() {
        let messages = vec![ProviderEvent::ToolResult {
            tool_use_id: Some("id1".into()),
            result: "ok".into(),
        }];
        let html = leptos::view! { <MessageStream messages=messages /> }.to_html();
        assert!(html.contains("ok"));
        assert!(html.contains("message-tool"));
        assert!(html.contains(r#"data-tool-use-id="id1""#));
    }

    #[test]
    fn test_message_stream_renders_thinking() {
        let messages = vec![ProviderEvent::Thinking {
            thinking: "hmm".into(),
        }];
        let html = leptos::view! { <MessageStream messages=messages /> }.to_html();
        assert!(html.contains("hmm"));
        assert!(html.contains("message-thinking"));
        assert!(html.contains("mdi-snowflake-outline"));
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

    #[test]
    fn test_message_stream_suppresses_text_chunk() {
        let messages = vec![ProviderEvent::TextChunk {
            delta: "partial".into(),
        }];
        let html = leptos::view! { <MessageStream messages=messages /> }.to_html();
        assert!(html.contains("message-stream"));
        assert!(!html.contains("partial"));
    }

    #[test]
    fn test_message_stream_suppresses_thinking_chunk() {
        let messages = vec![ProviderEvent::ThinkingChunk {
            delta: "thinking...".into(),
        }];
        let html = leptos::view! { <MessageStream messages=messages /> }.to_html();
        assert!(html.contains("message-stream"));
        assert!(!html.contains("thinking..."));
    }

    #[test]
    fn test_message_stream_text_shows_full_when_short() {
        let short = "x".repeat(400);
        let messages = vec![ProviderEvent::Text {
            text: short.clone(),
        }];
        let html = leptos::view! { <MessageStream messages=messages /> }.to_html();
        assert!(html.contains(&short));
        assert!(!html.contains("show-more-btn"));
    }

    #[test]
    fn test_message_stream_text_collapses_when_long() {
        let long = "x".repeat(401);
        let messages = vec![ProviderEvent::Text { text: long }];
        let html = leptos::view! { <MessageStream messages=messages /> }.to_html();
        assert!(html.contains("show-more-btn"));
    }

    #[test]
    fn test_message_stream_user_text_uses_css_class_no_inline_styles() {
        let messages = vec![ProviderEvent::UserText {
            text: "hello".into(),
        }];
        let html = leptos::view! { <MessageStream messages=messages /> }.to_html();
        assert!(html.contains("message-user"));
        assert!(!html.contains("background:#1565c0"));
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
    view! {
        <div id="message-stream" class="message-stream" style="padding:1rem;overflow-wrap:break-word">
            {if messages.is_empty() {
                view! { <p class="has-text-grey">"No messages yet. Start a conversation to see messages here."</p> }.into_any()
            } else {
                view! { <div inner_html=rendered /> }.into_any()
            }}
        </div>
    }
}
