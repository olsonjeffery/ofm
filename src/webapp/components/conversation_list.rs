use crate::db::schema::{ConversationWithRun, RunStatus};
use leptos::prelude::*;

fn run_status_class(status: &RunStatus) -> &'static str {
    match status {
        RunStatus::Pending => "is-light",
        RunStatus::Running => "is-info",
        RunStatus::Completed => "is-success",
        RunStatus::Failed => "is-danger",
        RunStatus::Blocked => "is-warning",
    }
}

fn run_status_label(status: &RunStatus) -> &'static str {
    match status {
        RunStatus::Pending => "Pending",
        RunStatus::Running => "Running",
        RunStatus::Completed => "Completed",
        RunStatus::Failed => "Failed",
        RunStatus::Blocked => "Blocked",
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
                        let agent_type_str = cwr.run.as_ref().map(|r| r.agent_type.to_string()).unwrap_or_default();
                        let status = cwr.run.as_ref().map(|r| &r.status);
                        let name = cwr.conversation.name.clone().unwrap_or_else(|| cwr.conversation.model.clone());
                        let created = cwr.conversation.created_at.format("%Y-%m-%d %H:%M").to_string();

                        view! {
                            <div class={card_class} style="margin-bottom:0.5rem;cursor:pointer"
                                data-conversation-id={conv_id.to_string()}
                                onclick="window.handleConversationClick(event)"
                            >
                                <div class="card-content" style="padding:0.5rem">
                                    <div class="level is-mobile" style="margin-bottom:0">
                                        <div class="level-left">
                                            <small><strong>{name.clone()}</strong></small>
                                        </div>
                                        <div class="level-right">
                                            {if let Some(s) = status {
                                                view! { <span class={format!("tag is-small {}", run_status_class(s))}>{run_status_label(s)}</span> }.into_any()
                                            } else {
                                                view! { <span class="tag is-small is-light">"Unknown"</span> }.into_any()
                                            }}
                                        </div>
                                    </div>
                                    <small class="has-text-grey">{agent_type_str} &middot; {created}</small>
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
    use crate::db::schema::{AgentType, Conversation, RunStatus, TaskAgentRun};
    use chrono::NaiveDateTime;

    fn make_conversation(id: uuid::Uuid, name: &str) -> Conversation {
        Conversation {
            id,
            task_id: 1,
            omp_session_id: Some("sess-1".into()),
            model: "gpt-4".into(),
            effort: "balanced".into(),
            name: Some(name.into()),
            created_at: NaiveDateTime::parse_from_str("2024-06-01 12:00:00", "%Y-%m-%d %H:%M:%S")
                .unwrap(),
        }
    }

    fn make_run(conv_id: uuid::Uuid) -> TaskAgentRun {
        TaskAgentRun {
            id: uuid::Uuid::new_v4(),
            task_id: 1,
            agent_type: AgentType::Implementation,
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
            run: Some(make_run(conv_id)),
        }];
        let html =
            leptos::view! { <ConversationList conversations=convs active_id=None /> }.to_html();
        assert!(html.contains("Test Chat"));
        assert!(html.contains("Completed"));
    }
}
