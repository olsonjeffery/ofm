use crate::db::schema::{AgentType, ConversationWithRun};
use leptos::prelude::*;

fn agent_icon(agent_type: &AgentType) -> &'static str {
    match agent_type {
        AgentType::Planification => "clipboard-list-outline",
        AgentType::Implementation => "code-tags",
        AgentType::Refinement => "tune",
        AgentType::Review => "eye-outline",
        AgentType::Pr => "source-pull",
        AgentType::Yolo => "rocket",
    }
}

fn is_valid_name(name: &str) -> bool {
    if name.len() < 3 {
        return false;
    }
    if name.starts_with("Generate a 1-3 word title") {
        return false;
    }
    if name.starts_with("generate a 1-3 word title") {
        return false;
    }
    true
}

fn format_conversation_date(created_at: &chrono::NaiveDateTime) -> String {
    let epoch = chrono::NaiveDateTime::parse_from_str("1970-01-01 00:00:00", "%Y-%m-%d %H:%M:%S")
        .unwrap_or_default();
    if *created_at <= epoch {
        return String::new();
    }
    let now = chrono::Utc::now().naive_utc();
    let diff = now - *created_at;
    if diff.num_minutes() < 1 {
        "Just now".to_string()
    } else if diff.num_hours() < 1 {
        format!("{}m ago", diff.num_minutes())
    } else if diff.num_days() < 1 {
        format!("{}h ago", diff.num_hours())
    } else if diff.num_days() < 7 {
        format!("{}d ago", diff.num_days())
    } else {
        created_at.format("%b %d").to_string()
    }
}

#[component]
pub fn ConversationList(
    conversations: Vec<ConversationWithRun>,
    active_id: Option<uuid::Uuid>,
) -> impl IntoView {
    view! {
        <div class="conversation-list">
            {if conversations.is_empty() {
                view! { <p class="has-text-grey is-size-7 p-3">"No conversations yet."</p> }.into_any()
            } else {
                view! {
                    {conversations.iter().map(|cwr| {
                        let conv_id = cwr.conversation.id;
                        let is_active = active_id.map(|id| id == conv_id).unwrap_or(false);
                        let card_class = if is_active { "card is-active" } else { "card" };
                        let agent_type = cwr.run.as_ref().map(|r| &r.agent_type);
                        let icon = agent_type.map(agent_icon).unwrap_or("chat-outline");
                        let raw_name = cwr.conversation.name.clone().unwrap_or_default();
                        let name = if is_valid_name(&raw_name) {
                            raw_name
                        } else {
                            cwr.conversation.model.clone()
                        };
                        let date_str = format_conversation_date(&cwr.conversation.created_at);

                        view! {
                            <div class={card_class} style="padding:0.75rem;margin-bottom:0.25rem;cursor:pointer"
                                data-conversation-id={conv_id.to_string()}
                                onclick="window.handleConversationClick(event)"
                            >
                                <div class="level is-mobile" style="margin-bottom:0">
                                    <div class="level-left" style="display:flex;align-items:center;gap:0.5rem">
                                        <span class="icon has-text-info">
                                            <i class={format!("mdi mdi-{}", icon)}></i>
                                        </span>
                                        <div>
                                            <strong>{name}</strong>
                                            <div class="has-text-grey is-size-7">{cwr.conversation.model.clone()}</div>
                                        </div>
                                    </div>
                                    <div class="level-right">
                                        <small class="has-text-grey conversation-date" data-conv-id={conv_id.to_string()}>
                                            {date_str}
                                        </small>
                                    </div>
                                </div>
                            </div>
                        }
                    }).collect::<Vec<_>>()}
                }.into_any()
            }}
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::schema::{Conversation, RunStatus, TaskAgentRun};
    use chrono::NaiveDateTime;

    fn make_conversation(id: uuid::Uuid, name: &str) -> Conversation {
        Conversation {
            id,
            task_id: 1,
            provider_session_id: Some("sess-1".into()),
            model: "gpt-4".into(),
            effort: "balanced".into(),
            name: Some(name.into()),
            created_at: NaiveDateTime::parse_from_str("2024-06-01 12:00:00", "%Y-%m-%d %H:%M:%S")
                .unwrap(),
        }
    }

    fn make_run(conv_id: uuid::Uuid, agent_type: AgentType) -> TaskAgentRun {
        TaskAgentRun {
            id: uuid::Uuid::new_v4(),
            task_id: 1,
            agent_type,
            status: RunStatus::Completed,
            conversation_id: Some(conv_id),
            created_at: NaiveDateTime::parse_from_str("2024-06-01 12:00:00", "%Y-%m-%d %H:%M:%S")
                .unwrap(),
            completed_at: None,
        }
    }

    #[test]
    fn test_conversation_list_empty() {
        let html = leptos::view! { <ConversationList conversations=Vec::new() active_id=None /> }
            .to_html();
        assert!(html.contains("No conversations yet"));
    }

    #[test]
    fn test_conversation_list_renders_items() {
        let conv_id = uuid::Uuid::new_v4();
        let convs = vec![ConversationWithRun {
            conversation: make_conversation(conv_id, "Test Chat"),
            run: Some(make_run(conv_id, AgentType::Implementation)),
        }];
        let html =
            leptos::view! { <ConversationList conversations=convs active_id=None /> }.to_html();
        assert!(html.contains("Test Chat"));
        assert!(html.contains("gpt-4"));
        assert!(html.contains("data-conv-id"));
        assert!(html.contains("mdi-code-tags"));
        assert!(html.contains("level-left"));
        assert!(html.contains("level-right"));
    }

    #[test]
    fn test_conversation_list_default_icon_no_run() {
        let conv_id = uuid::Uuid::new_v4();
        let convs = vec![ConversationWithRun {
            conversation: make_conversation(conv_id, "No Run Chat"),
            run: None,
        }];
        let html =
            leptos::view! { <ConversationList conversations=convs active_id=None /> }.to_html();
        assert!(html.contains("mdi-chat-outline"));
    }
}
