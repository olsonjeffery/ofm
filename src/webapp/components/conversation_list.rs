use crate::db::schema::{AgentType, ConversationWithRun, RunStatus};
use crate::webapp::components::datetime::utc_attr;
use leptos::prelude::*;

fn run_status_class(status: &RunStatus) -> &'static str {
    match status {
        RunStatus::Pending => "is-light",
        RunStatus::Running => "is-info is-light",
        RunStatus::Completed => "is-success is-light",
        RunStatus::Failed => "is-danger is-light",
        RunStatus::Blocked => "is-warning is-light",
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

pub fn is_valid_name(name: &str) -> bool {
    name.len() >= 3
        && !name.starts_with("Generate a 1-3 word title")
        && !name.starts_with("generate a 1-3 word title")
}

#[component]
pub fn ConversationList(
    conversations: Vec<ConversationWithRun>,
    active_id: Option<uuid::Uuid>,
    task_id: String,
) -> impl IntoView {
    let _ = &active_id;
    view! {
        <div class="conversation-list">
            <div class="buttons is-centered" id="agent-run-buttons">
                <button class="button level-item is-small is-light" data-task-id={task_id.clone()} disabled=false data-agent-type="planification" >
                    <span class="icon is-small is-info"><i class="mdi mdi-file-document-outline"></i></span> <span>"Plan"</span>
                </button>
                <button class="button level-item is-small is-light" data-task-id={task_id.clone()} disabled=false data-agent-type="implementation">
                    <span class="icon is-small is-purple"><i class="mdi mdi-code-tags"></i></span> <span>"Impl"</span>
                </button>
                <button class="button level-item is-small is-light" data-task-id={task_id.clone()} disabled=false data-agent-type="review">
                    <span class="icon is-small is-primary"><i class="mdi mdi-checkbox-marked-circle-outline"></i></span> <span>"Rev"</span>
                </button>
                <button class="button level-item is-small is-light" data-task-id={task_id.clone()} disabled=false data-agent-type="refinement" >
                    <span class="icon is-small is-danger"><i class="mdi mdi-creation-outline"></i></span> <span>"Ref"</span>
                </button>
                <button class="button level-item is-small is-light" data-task-id={task_id.clone()} disabled=false data-agent-type="pr" >
                    <span class="icon is-small is-success"><i class="mdi mdi-source-branch-plus"></i></span> <span>"PR"</span>
                </button>
            </div>
            {if conversations.is_empty() {
                view! { <p class="has-text-grey is-size-7 p-3">"No conversations yet."</p> }.into_any()
            } else {
                view! {
                    {conversations.iter().map(|cwr| {
                        let conv_id = cwr.conversation.id;
                        let agent_type = cwr.run.as_ref().map(|r| &r.agent_type);
                        let icon = agent_type.map(AgentType::icon).unwrap_or("chat-outline");
                        let raw_name = cwr.conversation.name.clone().unwrap_or_default();
                        let name = if is_valid_name(&raw_name) {
                            raw_name
                        } else {
                            cwr.conversation.model.clone()
                        };
                        let effective_ts = cwr.conversation.updated_at.max(cwr.conversation.created_at);
                        let date_str = effective_ts.format("%b %d, %H:%M").to_string();
                        let date_utc = utc_attr(&effective_ts);
                        let status = cwr.run.as_ref().map(|r| &r.status);
                        let curr_agent_color = match agent_type {
                            Some(AgentType::Planification) => "var(--bulma-info)",
                            Some(AgentType::Implementation) => "var(--bulma-purple)",
                            Some(AgentType::Review) => "var(--bulma-primary)",
                            Some(AgentType::Refinement) => "var(--bulma-danger)",
                            Some(AgentType::Pr) => "var(--bulma-success)",
                            _ => "var(--bulma-grey-dark)",
                        };

                        view! {
                            <div class="card"
                                data-conversation-id={conv_id.to_string()}
                                onclick="window.handleConversationClick(event)"
                            >
                                <div class="card-content" style="padding:0.5rem">
                                <div class="level is-mobile" style="margin-bottom:0">
                                    <div class="level-left" style="display:flex;align-items:center;gap:0.5rem;min-width:0;flex-shrink:1;overflow-wrap:break-word;word-break:break-word">
                                        <span class="icon" style={format!("flex-shrink:0;color:{};", curr_agent_color)}>
                                            <i class={format!("mdi mdi-{}", icon)}></i>
                                        </span>
                                        <div style="min-width:0;overflow-wrap:break-word;word-break:break-word">
                                            <strong style="overflow-wrap:break-word;word-break:break-word">{name}</strong>
                                            <div class="has-text-grey is-size-7">{cwr.conversation.model.clone()}</div>
                                        </div>
                                    </div>
                                    <div class="level-right" style="display:flex;flex-direction:column;align-items:flex-end;gap:0.15rem;flex-shrink:0">
                                        {status.map(|s| view! {
                                            <span class={format!("tag {}", run_status_class(s))}>{run_status_label(s)}</span>
                                        })}
                                        <span class="has-text-grey conversation-date" data-conv-id={conv_id.to_string()}
                                               data-utc={date_utc} data-utc-format="datetime"
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
            <script>
            {r#"

                // Agent run buttons
                var buttons=document.querySelectorAll('[data-task-id][data-agent-type]');
                buttons.forEach(function(btn){
                    btn.addEventListener('click',function(){
                        var taskId=btn.getAttribute('data-task-id');
                        var agentType=btn.getAttribute('data-agent-type');
                        btn.disabled=true;
                        btn.classList.add('is-loading');
                        apiCall('/api/tasks/'+taskId+'/agent-runs',{
                            method:'POST',
                            headers:{'Content-Type':'application/json'},
                            body:JSON.stringify({agent_type:agentType})
                        }).then(function(r){
                            if(r.status===409){showMessage('Agent already running for this task');}
                            else if(r.status===403){showMessage('Provider credentials missing');}
                            else if(r.ok){window.location.reload();}
                            else{showMessage('Error starting agent');}
                        }).finally(function(){
                            btn.disabled=false;
                            btn.classList.remove('is-loading');
                        });
                    });
                });
                "#}
            </script>
            </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::schema::{Conversation, RunStatus, TaskAgentRun};
    use chrono::NaiveDateTime;

    fn make_conversation(id: uuid::Uuid, name: &str) -> Conversation {
        let dt = NaiveDateTime::parse_from_str("2024-06-01 12:00:00", "%Y-%m-%d %H:%M:%S").unwrap();
        Conversation {
            id,
            task_id: 1,
            provider_session_id: Some("sess-1".into()),
            model: "gpt-4".into(),
            effort: "balanced".into(),
            name: Some(name.into()),
            created_at: dt,
            updated_at: dt,
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
        let html = leptos::view! { <ConversationList conversations=Vec::new() active_id=None task_id="1".to_owned() /> }
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
            leptos::view! { <ConversationList conversations=convs active_id=None task_id="1".to_owned() /> }.to_html();
        assert!(html.contains("Test Chat"));
        assert!(html.contains("gpt-4"));
        assert!(html.contains("data-conv-id"));
        assert!(html.contains("mdi-code-tags"));
        assert!(html.contains("level-left"));
        assert!(html.contains("level-right"));
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
            leptos::view! { <ConversationList conversations=convs active_id=None task_id="1".to_owned() /> }.to_html();
        assert!(html.contains("mdi-chat-outline"));
    }

    #[test]
    fn test_conversation_list_status_labels() {
        let conv_id = uuid::Uuid::new_v4();
        let convs = vec![ConversationWithRun {
            conversation: make_conversation(conv_id, "Running Chat"),
            run: Some(TaskAgentRun {
                status: RunStatus::Running,
                ..make_run(conv_id, AgentType::Planification)
            }),
        }];
        let html =
            leptos::view! { <ConversationList conversations=convs active_id=None task_id="1".to_owned() /> }.to_html();
        assert!(html.contains("Running"));
        assert!(html.contains("mdi-file-document-outline"));
    }

    #[test]
    fn test_conversation_list_items_use_card_structure() {
        let conv_id = uuid::Uuid::new_v4();
        let convs = vec![ConversationWithRun {
            conversation: make_conversation(conv_id, "Card Chat"),
            run: None,
        }];
        let html =
            leptos::view! { <ConversationList conversations=convs active_id=None task_id="1".to_owned() /> }.to_html();
        assert!(html.contains(r#"class="card""#), "should use card class");
        assert!(html.contains("card-content"), "should have card-content");
        assert!(
            !html.contains("box is-light"),
            "should not use box is-light"
        );
    }

    #[test]
    fn test_conversation_list_agent_buttons_have_is_small() {
        let html = leptos::view! { <ConversationList conversations=Vec::new() active_id=None task_id="1".to_owned() /> }
            .to_html();
        assert!(
            html.contains(r#"class="button level-item is-small is-light""#),
            "agent buttons should have is-small"
        );
        assert!(html.contains("Plan"), "should have Plan button");
        assert!(html.contains("Impl"), "should have Impl button");
        assert!(html.contains("Rev"), "should have Rev button");
        assert!(html.contains("Ref"), "should have Ref button");
        assert!(html.contains("PR"), "should have PR button");
    }

    #[test]
    fn test_conversation_list_date_format_absolute() {
        let conv_id = uuid::Uuid::new_v4();
        let dt = NaiveDateTime::parse_from_str("2024-06-15 14:30:00", "%Y-%m-%d %H:%M:%S").unwrap();
        let mut conv = make_conversation(conv_id, "Dated Chat");
        conv.created_at = dt;
        conv.updated_at = dt;
        let convs = vec![ConversationWithRun {
            conversation: conv,
            run: None,
        }];
        let html =
            leptos::view! { <ConversationList conversations=convs active_id=None task_id="1".to_owned() /> }.to_html();
        assert!(html.contains("Jun 15"));
        assert!(html.contains(r#"data-utc="2024-06-15T14:30:00Z""#));
        assert!(html.contains(r#"data-utc-format="datetime""#));
        assert!(!html.contains("ago"));
        assert!(!html.contains("Just now"));
    }

    #[test]
    fn test_conversation_date_utc_uses_max_updated_created() {
        let conv_id = uuid::Uuid::new_v4();
        let created =
            NaiveDateTime::parse_from_str("2024-06-10 08:00:00", "%Y-%m-%d %H:%M:%S").unwrap();
        let updated =
            NaiveDateTime::parse_from_str("2024-06-15 14:30:00", "%Y-%m-%d %H:%M:%S").unwrap();
        let mut conv = make_conversation(conv_id, "Dated Chat");
        conv.created_at = created;
        conv.updated_at = updated;
        let convs = vec![ConversationWithRun {
            conversation: conv,
            run: None,
        }];
        let html =
            leptos::view! { <ConversationList conversations=convs active_id=None task_id="1".to_owned() /> }.to_html();
        assert!(html.contains(r#"data-utc="2024-06-15T14:30:00Z""#));
        assert!(!html.contains(r#"data-utc="2024-06-10T08:00:00Z""#));
    }
}
