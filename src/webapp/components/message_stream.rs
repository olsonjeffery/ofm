use crate::providers::types::ProviderEvent;
use leptos::prelude::*;
use pulldown_cmark::Options;
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};

static COLLAPSE_ID: AtomicU64 = AtomicU64::new(0);

fn next_id() -> String {
    format!("c{}", COLLAPSE_ID.fetch_add(1, Ordering::Relaxed))
}

fn esc(s: &str) -> String {
    html_escape::encode_text(s).to_string()
}

fn render_markdown(text: &str) -> String {
    let escaped = html_escape::encode_text(text);
    let mut opt = Options::empty();
    opt.insert(Options::all());
    let parser = pulldown_cmark::Parser::new_ext(&escaped, opt);
    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, parser);
    ammonia::Builder::default()
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
        .to_string()
}

fn maybe_collapse(content: &str, html_id: &str) -> String {
    if content.len() <= 256 {
        esc(content)
    } else {
        let truncated_content = content.chars().take(256).collect::<String>();
        let truncated_lines = truncated_content.lines().count();
        let full_lines = content.lines().count();
        let more_lines = full_lines - truncated_lines;
        format!(
            r##"<pre id="preview-{}">{}</pre>
                <pre id="full-{}" style="display:none">{}</pre>
                <a href="#" id="btn-{}" class="show-more-btn" onclick="toggleShowMoreLines('{}', {});return false">show {} more lines</a>
            "##,
            esc(html_id),
            esc(&format!("{}…", truncated_content)),
            esc(html_id),
            esc(content),
            esc(html_id),
            esc(html_id),
            more_lines,
            more_lines,
        )
    }
}

fn maybe_collapse_md(content: &str, html_id: &str) -> String {
    if content.len() <= 256 {
        render_markdown(content)
    } else {
        let truncated_content: String = content.chars().take(256).collect();
        let more_lines = content.lines().count() - truncated_content.lines().count();
        format!(
            r##"<span id="preview-{}">{}</span><span id="full-{}" style="display:none">{}</span><a href="#" id="btn-{}" class="show-more-btn" onclick="toggleShowMoreLines('{}', {});return false">show {} more lines</a>"##,
            esc(html_id),
            render_markdown(&format!("{}…", truncated_content)),
            esc(html_id),
            render_markdown(content),
            esc(html_id),
            esc(html_id),
            more_lines,
            more_lines,
        )
    }
}

fn build_data_attrs(id_str: &str, message_id: &Option<String>) -> String {
    let mut attrs = String::new();
    if !id_str.is_empty() {
        attrs.push_str(&format!(r#" data-tool-use-id="{}""#, esc(id_str)));
    }
    if let Some(mid) = message_id {
        if !mid.is_empty() {
            attrs.push_str(&format!(r#" data-message-id="{}""#, esc(mid)));
        }
    }
    attrs
}

fn collapse_id(id_str: &str) -> String {
    if id_str.is_empty() {
        next_id()
    } else {
        id_str.to_string()
    }
}

pub fn render_event(event: &ProviderEvent) -> String {
    match event {
        ProviderEvent::Text { text } => {
            if text.trim().is_empty() {
                return String::new();
            }
            let id = next_id();
            let content = maybe_collapse_md(text, &id);
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
            message_id,
        } => {
            let input_str = serde_json::to_string_pretty(input).unwrap_or_default();
            let id_str = tool_use_id.as_deref().unwrap_or("");
            let content = maybe_collapse(&input_str, &collapse_id(id_str));
            let data_attrs = build_data_attrs(id_str, message_id);
            format!(
                r#"<div class="message-tool"{}><span class="icon"><i class="mdi mdi-cog-outline"></i></span> <code>{}</code>{}</div>"#,
                data_attrs,
                esc(tool_name),
                content
            )
        }
        ProviderEvent::ToolResult {
            tool_use_id,
            result,
            message_id,
        } => {
            let trimmed = result.trim();
            if trimmed.is_empty() || trimmed == "null" {
                return String::new();
            }
            let id_str = tool_use_id.as_deref().unwrap_or("");
            let content = maybe_collapse(result, &collapse_id(id_str));
            let data_attrs = build_data_attrs(id_str, message_id);
            format!(
                r#"<div class="message-tool"{}><span class="icon"><i class="mdi mdi-cog-outline"></i></span>{}</div>"#,
                data_attrs, content
            )
        }
        ProviderEvent::Thinking { thinking } => {
            let trimmed = thinking.trim();
            if trimmed.is_empty() {
                return String::new();
            }
            let id = next_id();
            let content = maybe_collapse_md(trimmed, &id);
            format!(
                r#"<div class="message-thinking"><span class="icon"><i class="mdi mdi-head-snowflake-outline"></i></span><div class="content">{}</div></div>"#,
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
                r#"<div class="message-user"><div class="content">{}</div></div>"#,
                render_markdown(text),
            )
        }
        ProviderEvent::Ready => String::new(),
        ProviderEvent::ExtensionUiRequest(_) => String::new(),
        ProviderEvent::AvailableCommandsUpdate(_) => String::new(),
        ProviderEvent::Response(data) => {
            let txt = data.as_str().unwrap_or("");
            if txt.trim().is_empty() {
                String::new()
            } else {
                let id = next_id();
                let content = maybe_collapse_md(txt, &id);
                format!(
                    r#"<div class="message-model"><div class="content">{}</div></div>"#,
                    content
                )
            }
        }
        ProviderEvent::QuestionAsked { ref questions, .. } => {
            if questions.is_empty() {
                return String::new();
            }
            let mut md = String::new();
            for q in questions {
                let hdr = q.header.as_deref().unwrap_or("Question");
                md.push_str(&format!("**{}**\n\n{}", esc(hdr), q.question));
                if !q.options.is_empty() {
                    md.push_str("\n\n");
                    for o in &q.options {
                        md.push_str(&format!(
                            "- **{}**{}",
                            esc(&o.label),
                            o.description
                                .as_deref()
                                .map(|d| format!(": {}", esc(d)))
                                .unwrap_or_default()
                        ));
                        md.push('\n');
                    }
                }
                md.push_str("\n\n");
            }
            let content = render_markdown(&md);
            format!(
                r#"<div class="message-question notification is-info is-light"><span class="icon"><i class="mdi mdi-help-circle-outline"></i></span>{}</div>"#,
                content
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
    use crate::providers::types::{AskedQuestion, ProviderEvent, QuestionOption};

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
            message_id: None,
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
            message_id: None,
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
        assert!(html.contains("mdi-head-snowflake-outline"));
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
        let short = "x".repeat(256);
        let messages = vec![ProviderEvent::Text {
            text: short.clone(),
        }];
        let html = leptos::view! { <MessageStream messages=messages /> }.to_html();
        assert!(html.contains(&short));
        assert!(!html.contains("show-more-btn"));
    }

    #[test]
    fn test_message_stream_text_collapses_when_long() {
        let long = "x".repeat(257);
        let messages = vec![ProviderEvent::Text { text: long }];
        let html = leptos::view! { <MessageStream messages=messages /> }.to_html();
        assert!(html.contains("show-more-btn"));
    }

    #[test]
    fn test_message_stream_md_collapse_has_ellipsis() {
        let long = "x".repeat(300);
        let messages = vec![ProviderEvent::Text { text: long.clone() }];
        let html = leptos::view! { <MessageStream messages=messages /> }.to_html();
        assert!(html.contains("show-more-btn"));
        assert!(html.contains("…"));
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

    #[test]
    fn test_message_stream_tool_result_suppresses_null() {
        let messages = vec![ProviderEvent::ToolResult {
            tool_use_id: Some("id1".into()),
            result: "null".into(),
            message_id: None,
        }];
        let html = leptos::view! { <MessageStream messages=messages /> }.to_html();
        assert!(!html.contains("pre"));
    }

    #[test]
    fn test_message_stream_tool_result_suppresses_empty() {
        let messages = vec![ProviderEvent::ToolResult {
            tool_use_id: Some("id1".into()),
            result: "".into(),
            message_id: None,
        }];
        let html = leptos::view! { <MessageStream messages=messages /> }.to_html();
        assert!(!html.contains("pre"));
    }

    #[test]
    fn test_message_stream_thinking_suppresses_empty() {
        let messages = vec![ProviderEvent::Thinking {
            thinking: "   ".into(),
        }];
        let html = leptos::view! { <MessageStream messages=messages /> }.to_html();
        assert!(!html.contains("message-thinking"));
    }

    #[test]
    fn test_message_stream_tool_use_has_data_message_id() {
        let messages = vec![ProviderEvent::ToolUse {
            tool_name: "read".into(),
            tool_use_id: Some("id1".into()),
            input: serde_json::json!({"path": "/tmp"}),
            message_id: Some("msg123".into()),
        }];
        let html = leptos::view! { <MessageStream messages=messages /> }.to_html();
        assert!(html.contains(r#"data-message-id="msg123""#));
    }

    #[test]
    fn test_message_stream_renders_tool_use_as_div() {
        let messages = vec![ProviderEvent::ToolUse {
            tool_name: "read".into(),
            tool_use_id: Some("id1".into()),
            input: serde_json::json!({"path": "/tmp"}),
            message_id: None,
        }];
        let html = leptos::view! { <MessageStream messages=messages /> }.to_html();
        assert!(html.contains("<div>"));
    }

    // ── Dedup tests ───────────────────────────────────────────────────────

    #[test]
    fn test_dedup_suppresses_duplicate_text() {
        let messages = vec![
            ProviderEvent::Text {
                text: "hello".into(),
            },
            ProviderEvent::Text {
                text: "hello".into(),
            },
        ];
        let html = leptos::view! { <MessageStream messages=messages /> }.to_html();
        assert_eq!(html.matches("hello").count(), 1);
    }

    #[test]
    fn test_dedup_suppresses_duplicate_tool_use() {
        let messages = vec![
            ProviderEvent::ToolUse {
                tool_name: "read".into(),
                tool_use_id: Some("id1".into()),
                input: serde_json::json!({"path": "/tmp"}),
                message_id: None,
            },
            ProviderEvent::ToolUse {
                tool_name: "read".into(),
                tool_use_id: Some("id1".into()),
                input: serde_json::json!({"path": "/tmp"}),
                message_id: None,
            },
        ];
        let html = leptos::view! { <MessageStream messages=messages /> }.to_html();
        assert_eq!(html.matches("read").count(), 1);
    }

    #[test]
    fn test_dedup_allows_different_tool_use() {
        let messages = vec![
            ProviderEvent::ToolUse {
                tool_name: "read".into(),
                tool_use_id: Some("id1".into()),
                input: serde_json::json!({"path": "/tmp"}),
                message_id: None,
            },
            ProviderEvent::ToolUse {
                tool_name: "write".into(),
                tool_use_id: Some("id2".into()),
                input: serde_json::json!({"path": "/tmp/test.txt"}),
                message_id: None,
            },
        ];
        let html = leptos::view! { <MessageStream messages=messages /> }.to_html();
        assert_eq!(html.matches("message-tool").count(), 2);
    }

    #[test]
    fn test_dedup_suppresses_duplicate_thinking() {
        let messages = vec![
            ProviderEvent::Thinking {
                thinking: "hmm".into(),
            },
            ProviderEvent::Thinking {
                thinking: "hmm".into(),
            },
        ];
        let html = leptos::view! { <MessageStream messages=messages /> }.to_html();
        assert_eq!(html.matches("hmm").count(), 1);
    }

    #[test]
    fn test_dedup_suppresses_duplicate_user_text() {
        let messages = vec![
            ProviderEvent::UserText {
                text: "hello".into(),
            },
            ProviderEvent::UserText {
                text: "hello".into(),
            },
        ];
        let html = leptos::view! { <MessageStream messages=messages /> }.to_html();
        assert_eq!(html.matches("hello").count(), 1);
    }

    // ── QuestionAsked rendering tests ─────────────────────────────────────

    #[test]
    fn test_question_asked_renders_notification_icon() {
        let messages = vec![ProviderEvent::QuestionAsked {
            session_id: "sess1".into(),
            questions: vec![AskedQuestion {
                question: "What model?".into(),
                header: Some("Choose".into()),
                options: vec![QuestionOption {
                    label: "gpt-4".into(),
                    description: Some("Fast".into()),
                }],
            }],
            tool_call_id: None,
            message_id: None,
        }];
        let html = leptos::view! { <MessageStream messages=messages /> }.to_html();
        assert!(html.contains("notification is-info is-light"));
        assert!(html.contains("mdi-help-circle-outline"));
        assert!(html.contains("message-question"));
    }

    #[test]
    fn test_question_asked_renders_fields_as_markdown() {
        let messages = vec![ProviderEvent::QuestionAsked {
            session_id: "sess1".into(),
            questions: vec![AskedQuestion {
                question: "Which color?".into(),
                header: Some("Pick".into()),
                options: vec![
                    QuestionOption {
                        label: "Red".into(),
                        description: Some("Warm".into()),
                    },
                    QuestionOption {
                        label: "Blue".into(),
                        description: None,
                    },
                ],
            }],
            tool_call_id: None,
            message_id: None,
        }];
        let html = leptos::view! { <MessageStream messages=messages /> }.to_html();
        assert!(html.contains("Pick"));
        assert!(html.contains("Which color?"));
        assert!(html.contains("Red"));
        assert!(html.contains("Warm"));
        assert!(html.contains("Blue"));
    }

    #[test]
    fn test_question_asked_empty_renders_nothing() {
        let messages = vec![ProviderEvent::QuestionAsked {
            session_id: "sess1".into(),
            questions: vec![],
            tool_call_id: None,
            message_id: None,
        }];
        let html = leptos::view! { <MessageStream messages=messages /> }.to_html();
        assert!(!html.contains("notification is-info is-light"));
    }

    #[test]
    fn test_user_text_still_renders_normally() {
        let messages = vec![ProviderEvent::UserText {
            text: "hello user".into(),
        }];
        let html = leptos::view! { <MessageStream messages=messages /> }.to_html();
        assert!(html.contains("hello user"));
        assert!(html.contains("message-user"));
    }
}

#[component]
pub fn MessageStream(messages: Vec<ProviderEvent>) -> impl IntoView {
    let mut seen = HashSet::new();
    let rendered: String = messages
        .iter()
        .filter(|event| {
            let key = match event {
                ProviderEvent::Text { text } => Some(format!("text:{text}")),
                ProviderEvent::UserText { text } => Some(format!("user_text:{text}")),
                ProviderEvent::Thinking { thinking } => Some(format!("thinking:{thinking}")),
                ProviderEvent::ToolUse {
                    tool_use_id,
                    message_id,
                    ..
                } => tool_use_id
                    .as_deref()
                    .or(message_id.as_deref())
                    .map(|s| s.to_string()),
                ProviderEvent::ToolResult {
                    tool_use_id,
                    message_id,
                    ..
                } => tool_use_id
                    .as_deref()
                    .or(message_id.as_deref())
                    .map(|s| s.to_string()),
                _ => None,
            };
            match key {
                Some(k) => seen.insert(k),
                None => true,
            }
        })
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
