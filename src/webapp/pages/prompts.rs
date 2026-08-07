use std::collections::HashMap;

use leptos::prelude::*;
use uuid::Uuid;

use crate::db::schema::Prompt;

fn truncate(content: &str, max: usize) -> String {
    let trimmed = content.trim();
    if trimmed.chars().count() <= max {
        trimmed.to_string()
    } else {
        let cut: String = trimmed.chars().take(max).collect();
        format!("{cut}…")
    }
}

#[component]
pub fn PromptsPage(
    prompts: Vec<Prompt>,
    user_id: Uuid,
    composition_summaries: HashMap<Uuid, String>,
) -> impl IntoView {
    let owned = |p: &Prompt| p.owner_user_id == Some(user_id) && !p.is_static;

    view! {
        <section class="section">
            <div class="level">
                <div class="level-left">
                    <h1 class="title">"Prompt Library"</h1>
                </div>
                <div class="level-right">
                    <div class="buttons">
                        <button id="new-snippet-btn" class="button is-small is-primary">
                            <span class="icon is-small"><i class="mdi mdi-plus"></i></span>
                            <span>"New Snippet"</span>
                        </button>
                        <button id="new-composite-btn" class="button is-small is-primary">
                            <span class="icon is-small"><i class="mdi mdi-file-tree"></i></span>
                            <span>"New Composite"</span>
                        </button>
                    </div>
                </div>
            </div>

            <div class="tabs is-small">
                <ul>
                    <li class="is-active" data-filter="all"><a>"All"</a></li>
                    <li data-filter="snippet"><a>"Snippets"</a></li>
                    <li data-filter="composite"><a>"Composites"</a></li>
                    <li data-filter="shared"><a>"Shared"</a></li>
                </ul>
            </div>

            {if prompts.is_empty() {
                view! {
                    <div class="box">
                        <p class="has-text-grey">"No prompts yet. Create a snippet to get started."</p>
                    </div>
                }.into_any()
            } else {
                view! {
                    <div class="columns is-multiline">
                        {prompts.into_iter().map(|prompt| {
                            let kind = prompt.kind.label();
                            let filter = if prompt.is_shared || prompt.is_static {
                                format!("{} shared", kind)
                            } else {
                                kind.to_string()
                            };
                            let summary = composition_summaries.get(&prompt.id).cloned().unwrap_or_default();
                            let editable = owned(&prompt);
                            let detail_url = format!("/webapp/prompts/{}", prompt.id);
                            view! {
                                <div class="column is-one-third" data-filter-cards={filter.clone()}>
                                    <div class="card">
                                        <div class="card-header">
                                            <p class="card-header-title">
                                                {prompt.title.clone()}
                                                {if prompt.is_static {
                                                    view! {
                                                        <span class="tag is-warning is-light is-small" style="margin-left:0.5rem" title="Built-in template">
                                                            <span class="icon is-small"><i class="mdi mdi-lock"></i></span>
                                                            <span>"static"</span>
                                                        </span>
                                                    }.into_any()
                                                } else { "".into_any() }}
                                            </p>
                                        </div>
                                        <div class="card-content" style="padding:0.75rem">
                                            <div class="tags">
                                                <span class="tag is-info is-light">{kind}</span>
                                                {prompt.tags.iter().map(|t| view! { <span class="tag">{t.clone()}</span> }).collect::<Vec<_>>()}
                                            </div>
                                            {if !summary.is_empty() {
                                                view! { <p class="is-size-7 has-text-grey">{summary}</p> }.into_any()
                                            } else { "".into_any() }}
                                            <p class="has-text-grey-dark" style="white-space:pre-wrap">{truncate(&prompt.content, 120)}</p>
                                        </div>
                                        <div class="card-footer">
                                            <a class="card-footer-item" href={detail_url.clone()}>
                                                <span class="icon is-small"><i class="mdi mdi-eye-outline"></i></span>
                                                <span>"View"</span>
                                            </a>
                                            <a class="card-footer-item" href={detail_url}>
                                                <span class="icon is-small"><i class="mdi mdi-pencil-outline"></i></span>
                                                <span>"Edit"</span>
                                            </a>
                                            <button
                                                class="card-footer-item button is-ghost"
                                                data-prompt-duplicate=""
                                                data-prompt-id={prompt.id.to_string()}
                                                title="Duplicate into my library"
                                            >
                                                <span class="icon is-small"><i class="mdi mdi-content-copy"></i></span>
                                                <span>"Duplicate"</span>
                                            </button>
                                            {if editable {
                                                view! {
                                                    <button
                                                        class="card-footer-item button is-ghost"
                                                        data-prompt-delete=""
                                                        data-prompt-id={prompt.id.to_string()}
                                                        title="Delete prompt"
                                                    >
                                                        <span class="icon is-small"><i class="mdi mdi-trash-can-outline"></i></span>
                                                        <span>"Delete"</span>
                                                    </button>
                                                }.into_any()
                                            } else { "".into_any() }}
                                        </div>
                                    </div>
                                </div>
                            }
                        }).collect::<Vec<_>>()}
                    </div>
                }.into_any()
            }}
        </section>
        <script>
            {r#"document.addEventListener('DOMContentLoaded',function(){
                function create(kind){
                    apiCall('/api/prompts',{method:'POST',headers:{'Content-Type':'application/json'},
                        body:JSON.stringify({kind:kind,title:'Untitled '+(kind==='composite'?'composite':'snippet'),content:'',tags:[],is_shared:false,children:[]})})
                        .then(function(r){return r.json();})
                        .then(function(d){window.location.href='/webapp/prompts/'+d.id;});
                }
                var newSnippet=document.getElementById('new-snippet-btn');
                var newComposite=document.getElementById('new-composite-btn');
                if(newSnippet)newSnippet.addEventListener('click',function(){create('snippet');});
                if(newComposite)newComposite.addEventListener('click',function(){create('composite');});

                var tabs=document.querySelectorAll('.tabs li[data-filter]');
                tabs.forEach(function(tab){
                    tab.addEventListener('click',function(){
                        tabs.forEach(function(t){t.classList.remove('is-active');});
                        tab.classList.add('is-active');
                        var filter=tab.getAttribute('data-filter');
                        document.querySelectorAll('[data-filter-cards]').forEach(function(card){
                            var kinds=card.getAttribute('data-filter-cards');
                            if(filter==='all'){
                                card.style.display='';
                            }else if(filter==='shared'){
                                card.style.display=kinds.indexOf('shared')!==-1?'':'none';
                            }else{
                                card.style.display=kinds.indexOf(filter)!==-1?'':'none';
                            }
                        });
                    });
                });

                document.addEventListener('click',function(e){
                    var dupBtn=e.target.closest('[data-prompt-duplicate]');
                    if(dupBtn){
                        e.preventDefault();
                        e.stopPropagation();
                        var id=dupBtn.getAttribute('data-prompt-id');
                        apiCall('/api/prompts/'+id+'/duplicate',{method:'POST'})
                            .then(function(r){if(r.ok)window.location.reload();});
                        return;
                    }
                    var delBtn=e.target.closest('[data-prompt-delete]');
                    if(delBtn){
                        e.preventDefault();
                        e.stopPropagation();
                        var id=delBtn.getAttribute('data-prompt-id');
                        if(!confirm('Delete this prompt?'))return;
                        apiCall('/api/prompts/'+id,{method:'DELETE'})
                            .then(function(r){if(r.ok)window.location.reload();});
                    }
                });
            });"#}
        </script>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::schema::PromptKind;

    fn make_prompt(id: &str, kind: PromptKind, title: &str) -> Prompt {
        Prompt {
            id: Uuid::parse_str(id).unwrap(),
            kind,
            title: title.into(),
            content: "content {{taskId}}".into(),
            tags: vec!["desktop-3d".into()],
            owner_user_id: Some(Uuid::new_v4()),
            is_static: false,
            is_shared: false,
            static_key: None,
            created_at: chrono::NaiveDateTime::parse_from_str(
                "2024-01-15 10:00:00",
                "%Y-%m-%d %H:%M:%S",
            )
            .unwrap(),
            updated_at: chrono::NaiveDateTime::parse_from_str(
                "2024-01-15 10:00:00",
                "%Y-%m-%d %H:%M:%S",
            )
            .unwrap(),
        }
    }

    #[test]
    fn test_prompts_page_renders_cards_and_pills() {
        let owner = Uuid::new_v4();
        let mut p = make_prompt(
            "00000000-0000-0000-0000-000000000001",
            PromptKind::Snippet,
            "My Snippet",
        );
        p.owner_user_id = Some(owner);
        let html = leptos::view! {
            <PromptsPage prompts=vec![p] user_id=owner composition_summaries=HashMap::new() />
        }
        .to_html();
        assert!(html.contains("Prompt Library"));
        assert!(html.contains("My Snippet"));
        assert!(html.contains("desktop-3d"));
        assert!(html.contains("/webapp/prompts/00000000-0000-0000-0000-000000000001"));
        assert!(html.contains("Duplicate"));
        assert!(html.contains("New Snippet"));
        assert!(html.contains("New Composite"));
    }

    #[test]
    fn test_static_prompt_shows_badge_and_no_delete() {
        let owner = Uuid::new_v4();
        let mut p = make_prompt(
            "00000000-0000-0000-0000-000000000002",
            PromptKind::Static,
            "Implementation",
        );
        p.is_static = true;
        p.owner_user_id = None;
        let html = leptos::view! {
            <PromptsPage prompts=vec![p] user_id=owner composition_summaries=HashMap::new() />
        }
        .to_html();
        assert!(html.contains("static"));
        assert!(html.contains("mdi-lock"));
        assert!(
            !html.contains(r#"data-prompt-delete="""#),
            "static prompts must not render a delete button"
        );
    }
}
