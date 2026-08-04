use crate::db::schema::AgentType;
use leptos::prelude::*;

#[component]
pub fn ChatStatusBar(
    agent_type: Option<AgentType>,
    model_label: String,
    processing: bool,
) -> impl IntoView {
    let status_text = if processing {
        "Agent is processing..."
    } else {
        "Agent Idle"
    };

    let is_processing_class = if processing { "is-processing" } else { "" };

    view! {
        <div id="chat-status-bar"
             class=format!("chat-status-bar {}", is_processing_class)
             aria-live="polite">
            <div class="chat-status-info">
                {match agent_type {
                    Some(agent_type) => {
                        let type_str = agent_type.to_string();
                        let type_class = format!("is-agent-{}", type_str);
                        let icon_class = if processing {
                            format!("mdi mdi-{} agent-status-icon is-pulse", agent_type.icon())
                        } else {
                            format!("mdi mdi-{} agent-status-icon", agent_type.icon())
                        };
                        view! {
                            <span class=format!("chat-status-agent {}", type_class)>
                                <span class="icon">
                                    <i class=icon_class></i>
                                </span>
                                <span class="agent-type-label">{agent_type.label()}</span>
                            </span>
                        }
                            .into_any()
                    }
                    None => ().into_any(),
                }}
                {if model_label.is_empty() {
                    ().into_any()
                } else {
                    view! { <span class="agent-model-label">{model_label.clone()}</span> }
                        .into_any()
                }}
            </div>
            <div class="chat-status-actions">
                <span id="agent-status-label">{status_text}</span>
                <button id="stop-agent-btn"
                        class="button is-primary has-text-white is-small"
                        disabled={!processing}
                        onclick="stopAgent()">
                    <span class="icon is-small"><i class="mdi mdi-close-thick"></i></span>
                    <span>"Stop Agent"</span>
                </button>
            </div>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(agent_type: Option<AgentType>, model_label: &str, processing: bool) -> String {
        leptos::view! {
            <ChatStatusBar
                agent_type=agent_type
                model_label=model_label.to_string()
                processing=processing
            />
        }
        .to_html()
    }

    #[test]
    fn test_idle_render() {
        let html = render(Some(AgentType::Implementation), "gpt-4", false);
        assert!(html.contains("chat-status-bar"), "bar root present");
        assert!(html.contains("Agent Idle"));
        assert!(html.contains("Stop Agent"));
        assert!(html.contains("disabled"), "stop button disabled when idle");
        assert!(
            !html.contains("is-processing"),
            "no processing class when idle"
        );
        assert!(!html.contains("is-pulse"), "no pulse when idle");
        assert!(!html.contains("Agent is processing..."));
    }

    #[test]
    fn test_processing_render() {
        let html = render(Some(AgentType::Implementation), "gpt-4", true);
        assert!(html.contains("chat-status-bar"));
        assert!(html.contains("Agent is processing..."));
        assert!(html.contains("Stop Agent"));
        assert!(
            !html.contains("disabled"),
            "stop button enabled when processing"
        );
        assert!(html.contains("is-processing"), "processing class applied");
        assert!(html.contains("is-pulse"), "pulsing icon when processing");
        assert!(!html.contains("Agent Idle"));
    }

    #[test]
    fn test_agent_type_render() {
        let html = render(Some(AgentType::Implementation), "gpt-4", false);
        assert!(html.contains("mdi-code-tags"), "implementation icon");
        assert!(html.contains("Implementation"), "human agent label");
        assert!(
            html.contains("is-agent-implementation"),
            "per-type color class"
        );
        assert!(html.contains("gpt-4"), "model label shown");
        assert!(html.contains("agent-status-icon"));
    }

    #[test]
    fn test_no_agent_type_render() {
        let html = render(None, "gpt-4", false);
        assert!(html.contains("chat-status-bar"));
        assert!(html.contains("gpt-4"), "model label still shown");
        assert!(!html.contains("chat-status-agent"), "no agent block");
        assert!(!html.contains("agent-type-label"), "no agent label");
        assert!(html.contains("Agent Idle"));
    }

    #[test]
    fn test_empty_model_label_excluded() {
        let html = render(None, "", false);
        assert!(html.contains("chat-status-bar"));
        assert!(!html.contains("agent-model-label"), "no model label span");
    }
}
