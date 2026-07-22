use leptos::prelude::*;

#[component]
pub fn ChatInput(
    _on_send: leptos::prelude::Callback<String>,
    disabled: bool,
    _active_conversation_id: Option<uuid::Uuid>,
    task_id: i64,
) -> impl IntoView {
    let task_id_str = task_id.to_string();

    view! {
        <form id="chat-form" data-task-id={task_id_str}
              style="display:flex;gap:0.5rem;align-items:stretch;width:100%">
            <textarea
                class="textarea"
                id="chat-message-input"
                placeholder="Type your message..."
                rows="4"
                disabled=disabled
                style="flex:1;min-width:0"
            ></textarea>
            <button
                class="button is-primary has-text-white is-medium"
                id="chat-send-btn"
                disabled=disabled
                style="height:auto;align-self:stretch;display:flex;flex-direction:column;justify-content:flex-end;padding-bottom:0.5rem;padding-top:0.5rem;writing-mode:sideways-rl;"
            >
                "Send"
            </button>
        </form>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chat_input_renders() {
        let html = leptos::view! {
            <ChatInput
                _on_send=Callback::new(|_: String| {})
                disabled=false
                _active_conversation_id=None
                task_id=42
            />
        }
        .to_html();
        assert!(html.contains("Send"));
        assert!(html.contains("chat-form"));
        assert!(html.contains("data-task-id=\"42\""));
        assert!(
            !html.contains("has-addons"),
            "has-addons class should be removed"
        );
        assert!(html.contains("flex:1"), "textarea should be flex:1");
        assert!(html.contains("display:flex"), "form should use flex layout");
    }

    #[test]
    fn test_chat_input_disabled() {
        let html = leptos::view! {
            <ChatInput
                _on_send=Callback::new(|_: String| {})
                disabled=true
                _active_conversation_id=None
                task_id=1
            />
        }
        .to_html();
        assert!(html.contains("disabled"));
    }
}
