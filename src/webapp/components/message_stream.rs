use crate::providers::types::ProviderEvent;
use leptos::prelude::*;

fn esc(s: &str) -> String {
    html_escape::encode_text(s).to_string()
}

fn render_event(event: &ProviderEvent) -> String {
    match event {
        ProviderEvent::Text { text } => {
            format!(
                r#"<div class="box message-text"><div class="content">{}</div></div>"#,
                esc(text)
            )
        }
        ProviderEvent::TextChunk { delta } => {
            format!(r#"<span class="message-chunk">{}</span>"#, esc(delta))
        }
        ProviderEvent::ToolUse {
            tool_name,
            tool_use_id,
            input,
        } => {
            let input_str = serde_json::to_string_pretty(input).unwrap_or_default();
            let id_str = tool_use_id.as_deref().unwrap_or("");
            format!(
                r#"<div class="card"><div class="card-content"><span class="tag is-info is-light">{}</span> <code>{}</code><pre style="white-space:pre-wrap;word-break:break-word;overflow-wrap:break-word;max-width:100%">{}</pre></div></div>"#,
                esc(tool_name),
                esc(id_str),
                esc(&input_str)
            )
        }
        ProviderEvent::ToolResult {
            tool_use_id,
            result,
        } => {
            let id_str = tool_use_id.as_deref().unwrap_or("").to_string();
            let has_id = !id_str.is_empty();
            let truncated = has_id && result.len() > 100;
            if truncated {
                let preview = format!(
                    r#"<pre class="tool-result-preview" id="result-preview-{}" style="white-space:pre-wrap;word-break:break-word;overflow-wrap:break-word;max-width:100%">{}</pre>"#,
                    esc(&id_str),
                    esc(&result[..100])
                );
                let extra = format!(
                    r##"<a href="#" class="toggle-result" data-tool-id="{}" onclick="toggleResult(this);return false">show more</a>"##,
                    esc(&id_str)
                );
                let full = format!(
                    r#"<div class="tool-result-full" id="result-full-{}" style="display:none"><pre style="white-space:pre-wrap;word-break:break-word;overflow-wrap:break-word;max-width:100%">{}</pre></div>"#,
                    esc(&id_str),
                    esc(result)
                );
                format!(
                    r#"<div class="card"><div class="card-content"><span class="tag is-success is-light">result</span> <code>{}</code>{}{}{}</div></div>"#,
                    esc(&id_str),
                    preview,
                    extra,
                    full
                )
            } else {
                format!(
                    r#"<div class="card"><div class="card-content"><span class="tag is-success is-light">result</span> <code>{}</code><pre style="white-space:pre-wrap;word-break:break-word;overflow-wrap:break-word;max-width:100%">{}</pre></div></div>"#,
                    esc(&id_str),
                    esc(result)
                )
            }
        }
        ProviderEvent::Thinking { thinking } => {
            format!(
                r#"<div class="box message-thinking"><em style="color:#888;">{}</em></div>"#,
                esc(thinking)
            )
        }
        ProviderEvent::ThinkingChunk { delta } => {
            format!(
                r#"<span style="color:#888;font-style:italic;">{}</span>"#,
                esc(delta)
            )
        }
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
                r#"<content class="content message-user" style="display:block;background:#1565c0;color:#fff;padding:0.75rem;border-radius:6px;white-space:pre-wrap;max-width:33%;margin-left:auto">{}</content>"#,
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
                format!(
                    r#"<div class="box message-text"><div class="content">{}</div></div>"#,
                    esc(txt)
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
