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
        <div class="chat-input" style="border-top:1px solid #ddd;padding:1rem;background:#fff">
            <form id="chat-form" data-task-id={task_id_str}>
                <textarea
                    class="textarea"
                    id="chat-message-input"
                    placeholder="Type your message..."
                    rows="2"
                    disabled=disabled
                    style="width:100%;margin-bottom:0.5rem"
                ></textarea>
                <div style="display:flex;justify-content:flex-end">
                    <button
                        class="button is-primary"
                        id="chat-send-btn"
                        disabled=disabled
                    >
                        "Send"
                    </button>
                </div>
            </form>
        </div>
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
