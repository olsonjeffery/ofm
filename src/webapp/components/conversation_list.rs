use crate::db::schema::{AgentType, ConversationWithRun};
use leptos::prelude::*;

fn agent_icon(agent_type: &AgentType) -> &'static str {
    match agent_type {
        AgentType::Planification => "file-document-outline",
        AgentType::Implementation => "code-tags",
        AgentType::Refinement => "creation-outline",
        AgentType::Review => "checkbox-marked-circle-outline",
        AgentType::Pr => "source-branch-plus",
        AgentType::Yolo => "rocket",
    }
}

fn run_status_class(status: &crate::db::schema::RunStatus) -> &'static str {
    match status {
        crate::db::schema::RunStatus::Pending => "is-light",
        crate::db::schema::RunStatus::Running => "is-info is-light",
        crate::db::schema::RunStatus::Completed => "is-success is-light",
        crate::db::schema::RunStatus::Failed => "is-danger is-light",
        crate::db::schema::RunStatus::Blocked => "is-warning is-light",
    }
}

fn run_status_label(status: &crate::db::schema::RunStatus) -> &'static str {
    match status {
        crate::db::schema::RunStatus::Pending => "Pending",
        crate::db::schema::RunStatus::Running => "Running",
        crate::db::schema::RunStatus::Completed => "Completed",
        crate::db::schema::RunStatus::Failed => "Failed",
        crate::db::schema::RunStatus::Blocked => "Blocked",
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
    created_at.format("%b %d, %H:%M").to_string()
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
                        let agent_type = cwr.run.as_ref().map(|r| &r.agent_type);
                        let icon = agent_type.map(agent_icon).unwrap_or("chat-outline");
                        let raw_name = cwr.conversation.name.clone().unwrap_or_default();
                        let name = if is_valid_name(&raw_name) {
                            raw_name
                        } else {
                            cwr.conversation.model.clone()
                        };
                        let date_str = format_conversation_date(&cwr.conversation.created_at);
                        let status = cwr.run.as_ref().map(|r| &r.status);
                        let active_style = if is_active { "border-color:var(--bulma-primary)" } else { "" };

                        view! {
                            <div class="box is-info is-light" style={format!("padding:0.4rem;margin-bottom:0.25rem;cursor:pointer;overflow-wrap:break-word;word-break:break-word;{}", active_style)}
                                data-conversation-id={conv_id.to_string()}
                                onclick="window.handleConversationClick(event)"
                            >
                                <div class="level is-mobile" style="margin-bottom:0">
                                    <div class="level-left" style="display:flex;align-items:center;gap:0.5rem;min-width:0;flex-shrink:1;overflow-wrap:break-word;word-break:break-word">
                                        <span class="icon has-text-info" style="flex-shrink:0">
                                            <i class={format!("mdi mdi-{}", icon)}></i>
                                        </span>
                                        <div style="min-width:0;overflow-wrap:break-word;word-break:break-word">
                                            <strong style="overflow-wrap:break-word;word-break:break-word">{name}</strong>
                                            <div class="has-text-grey is-size-7">{cwr.conversation.model.clone()}</div>
                                        </div>
                                    </div>
                                    <div class="level-right" style="display:flex;flex-direction:column;align-items:flex-end;gap:0.15rem;flex-shrink:0">
                                        {status.map(|s| view! {
                                        <div>
                                            <span class={format!("tag {}", run_status_class(s))}>{run_status_label(s)}</span>
                                        </div>
                                        })}
                                        <div>
                                            <span class="has-text-grey conversation-date" data-conv-id={conv_id.to_string()}
                                                   style="white-space:nowrap;font-size:0.65rem">
                                                {date_str}
                                            </span>
                                        </div>
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
        assert!(html.contains("box is-info is-light"));
        assert!(html.contains("Completed"));
        assert!(html.contains("is-light"));
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

    #[test]
    fn test_conversation_list_status_labels() {
        use crate::db::schema::RunStatus;
        let conv_id = uuid::Uuid::new_v4();
        let convs = vec![ConversationWithRun {
            conversation: make_conversation(conv_id, "Running Chat"),
            run: Some(TaskAgentRun {
                status: RunStatus::Running,
                ..make_run(conv_id, AgentType::Planification)
            }),
        }];
        let html =
            leptos::view! { <ConversationList conversations=convs active_id=None /> }.to_html();
        assert!(html.contains("Running"));
        assert!(html.contains("mdi-file-document-outline"));
    }

    #[test]
    fn test_conversation_list_date_format_absolute() {
        let conv_id = uuid::Uuid::new_v4();
        let mut conv = make_conversation(conv_id, "Dated Chat");
        conv.created_at =
            NaiveDateTime::parse_from_str("2024-06-15 14:30:00", "%Y-%m-%d %H:%M:%S").unwrap();
        let convs = vec![ConversationWithRun {
            conversation: conv,
            run: None,
        }];
        let html =
            leptos::view! { <ConversationList conversations=convs active_id=None /> }.to_html();
        assert!(html.contains("Jun 15"));
        assert!(!html.contains("ago"));
        assert!(!html.contains("Just now"));
    }
}
