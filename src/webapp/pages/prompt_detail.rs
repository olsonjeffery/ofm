use leptos::prelude::*;
use uuid::Uuid;

use crate::db::schema::{Prompt, PromptAssignment, PromptKind};

fn kind_label(kind: &PromptKind) -> &'static str {
    match kind {
        PromptKind::Snippet => "snippet",
        PromptKind::Composite => "composite",
        PromptKind::Static => "static",
    }
}

#[component]
pub fn PromptDetailPage(
    prompt: Prompt,
    children: Vec<Prompt>,
    library: Vec<Prompt>,
    assignments: Vec<PromptAssignment>,
    project_options: Vec<(i64, String)>,
    user_id: Uuid,
) -> impl IntoView {
    let is_static = prompt.is_static;
    let owned = prompt.owner_user_id == Some(user_id);
    let is_composite = prompt.kind == PromptKind::Composite;
    let kind = kind_label(&prompt.kind);

    let children_json = serde_json::to_string(
        &children
            .iter()
            .map(|c| serde_json::json!({ "id": c.id.to_string(), "title": c.title, "content": c.content }))
            .collect::<Vec<_>>(),
    )
    .unwrap_or_else(|_| "[]".into());
    let library_json = serde_json::to_string(
        &library
            .iter()
            .filter(|l| l.id != prompt.id)
            .map(|l| serde_json::json!({ "id": l.id.to_string(), "title": l.title, "content": l.content }))
            .collect::<Vec<_>>(),
    )
    .unwrap_or_else(|_| "[]".into());
    let tags_value = prompt.tags.join(", ");

    view! {
        <section class="section">
            <div class="level">
                <div class="level-left">
                    <h1 class="title">"Prompt Builder"</h1>
                </div>
                <div class="level-right">
                    <div class="buttons">
                        {if !is_static {
                            view! {
                                <button id="duplicate-btn" class="button is-small is-info">
                                    <span class="icon is-small"><i class="mdi mdi-content-copy"></i></span>
                                    <span>"Duplicate"</span>
                                </button>
                                <button id="save-prompt-btn" class="button is-small is-success">
                                    <span class="icon is-small"><i class="mdi mdi-content-save"></i></span>
                                    <span>"Save"</span>
                                </button>
                                {if owned {
                                    view! {
                                        <button id="delete-prompt-btn" class="button is-small is-danger">
                                            <span class="icon is-small"><i class="mdi mdi-trash-can"></i></span>
                                            <span>"Delete"</span>
                                        </button>
                                    }.into_any()
                                } else { "".into_any() }}
                            }.into_any()
                        } else { "".into_any() }}
                    </div>
                </div>
            </div>

            <div class="box">
                <div class="field">
                    <label class="label">"Title"</label>
                    <div class="control">
                        <input
                            id="prompt-title"
                            class="input"
                            type="text"
                            value={prompt.title.clone()}
                            disabled=is_static
                        />
                    </div>
                </div>
                <div class="field">
                    <label class="label">"Tags"</label>
                    <div class="control">
                        <input
                            id="prompt-tags"
                            class="input"
                            type="text"
                            value={tags_value}
                            placeholder="comma-separated dash-based tags, e.g. desktop-3d, tests"
                            disabled=is_static
                        />
                    </div>
                    <div class="tags" style="margin-top:0.5rem">
                        <span class="tag is-info is-light">{kind}</span>
                        {if prompt.is_shared && !is_static {
                            view! { <span class="tag is-success is-light">"shared"</span> }.into_any()
                        } else { "".into_any() }}
                        {prompt.tags.iter().map(|t| view! { <span class="tag">{t.clone()}</span> }).collect::<Vec<_>>()}
                    </div>
                </div>
                {if !is_static {
                    view! {
                        <div class="field">
                            <label class="label">"Share with all users"</label>
                            <div class="control">
                                <input
                                    id="share-toggle"
                                    type="checkbox"
                                    checked=prompt.is_shared
                                />
                            </div>
                        </div>
                    }.into_any()
                } else { "".into_any() }}
                <div class="field">
                    <label class="label">"Content"</label>
                    <div class="control">
                        <textarea
                            id="prompt-content"
                            class="textarea"
                            style="font-family:monospace"
                            rows="14"
                            disabled=is_static
                        >{prompt.content.clone()}</textarea>
                    </div>
                    <div class="buttons" style="margin-top:0.5rem">
                        <button id="validate-btn" class="button is-small is-primary">
                            <span class="icon is-small"><i class="mdi mdi-check-decagram"></i></span>
                            <span>"Validate"</span>
                        </button>
                    </div>
                    <div id="validation-result" class="notification is-danger is-hidden" style="margin-top:0.5rem"></div>
                </div>
            </div>

            {if is_composite {
                view! {
                    <div class="box">
                        <h3 class="title is-5">"Composition"</h3>
                        <div id="child-list">
                            {children.iter().map(|c| {
                                let row = serde_json::json!({
                                    "id": c.id.to_string(),
                                    "title": c.title,
                                    "content": c.content,
                                });
                                view! {
                                    <div class="level is-mobile" data-child-id={c.id.to_string()}
                                        data-child-entry={serde_json::to_string(&row).unwrap_or_default()}>
                                        <div class="level-left">
                                            <span class="icon has-text-grey"><i class="mdi mdi-grip-vertical"></i></span>
                                            <span>{c.title.clone()}</span>
                                        </div>
                                        <div class="level-right">
                                            <div class="buttons are-small">
                                                <button class="button is-light" data-child-up title="Move up"><span class="icon is-small"><i class="mdi mdi-arrow-up"></i></span></button>
                                                <button class="button is-light" data-child-down title="Move down"><span class="icon is-small"><i class="mdi mdi-arrow-down"></i></span></button>
                                                <button class="button is-danger is-light" data-child-remove title="Remove"><span class="icon is-small"><i class="mdi mdi-close"></i></span></button>
                                            </div>
                                        </div>
                                    </div>
                                }
                            }).collect::<Vec<_>>()}
                        </div>
                        <div class="field has-addons" style="margin-top:0.75rem">
                            <div class="control is-expanded">
                                <div class="select is-fullwidth">
                                    <select id="add-child-select"></select>
                                </div>
                            </div>
                            <div class="control">
                                <button id="add-child-btn" class="button is-small is-primary">
                                    <span class="icon is-small"><i class="mdi mdi-plus"></i></span>
                                    <span>"Add"</span>
                                </button>
                            </div>
                        </div>
                        <label class="label" style="margin-top:1rem">"Preview (separated by ---)"</label>
                        <pre id="composite-preview" class="box" style="white-space:pre-wrap"></pre>
                    </div>
                }.into_any()
            } else { "".into_any() }}

            <div class="box">
                <h3 class="title is-5">"Designate"</h3>
                <p class="is-size-7 has-text-grey" style="margin-bottom:0.75rem">
                    "Designate this prompt to replace the stock template for an agent phase, at a project or globally."
                </p>
                <div class="field is-grouped">
                    <div class="control">
                        <div class="select">
                            <select id="designate-agent-type">
                                <option value="planification">"Planification"</option>
                                <option value="implementation">"Implementation"</option>
                                <option value="review">"Review"</option>
                                <option value="refinement">"Refinement"</option>
                                <option value="pr">"Pull Request"</option>
                            </select>
                        </div>
                    </div>
                    <div class="control">
                        <label class="radio"><input type="radio" name="designate-scope" value="global" checked />"Global"</label>
                        <label class="radio"><input type="radio" name="designate-scope" value="project" />"Project"</label>
                    </div>
                    <div class="control">
                        <div class="select" id="project-select-wrap">
                            <select id="designate-project">
                                {project_options.iter().map(|(id, name)| view! {
                                    <option value={id.to_string()}>{name.clone()}</option>
                                }).collect::<Vec<_>>()}
                            </select>
                        </div>
                    </div>
                    <div class="control">
                        <button id="designate-btn" class="button is-small is-primary">
                            <span class="icon is-small"><i class="mdi mdi-target"></i></span>
                            <span>"Designate"</span>
                        </button>
                    </div>
                </div>
                {if assignments.is_empty() {
                    view! { <p class="is-size-7 has-text-grey">"No designations yet."</p> }.into_any()
                } else {
                    view! {
                        <table class="table is-fullwidth is-hoverable" style="margin-top:0.75rem">
                            <thead>
                                <tr><th>"Agent"</th><th>"Scope"</th><th>"Project"</th><th></th></tr>
                            </thead>
                            <tbody>
                                {assignments.iter().map(|a| {
                                    let project_name = project_options.iter()
                                        .find(|(id, _)| Some(*id) == a.project_id)
                                        .map(|(_, name)| name.clone())
                                        .unwrap_or_else(|| a.project_id.map(|p| format!("#{p}")).unwrap_or_default());
                                    view! {
                                        <tr data-assignment-id={a.id.to_string()}>
                                            <td>{a.agent_type.label()}</td>
                                            <td>{a.scope_type.to_string()}</td>
                                            <td>{project_name}</td>
                                            <td>
                                                <button class="button is-small is-danger is-light" data-assignment-delete title="Remove designation">
                                                    <span class="icon is-small"><i class="mdi mdi-close"></i></span>
                                                </button>
                                            </td>
                                        </tr>
                                    }
                                }).collect::<Vec<_>>()}
                            </tbody>
                        </table>
                    }.into_any()
                }}
            </div>
        </section>
        <script>
            {r#"document.addEventListener('DOMContentLoaded',function(){
                var promptId=window.location.pathname.split('/').pop();
                var isStatic=document.getElementById('prompt-title')&&document.getElementById('prompt-title').disabled;

                var validationResult=document.getElementById('validation-result');
                var validateBtn=document.getElementById('validate-btn');
                if(validateBtn)validateBtn.addEventListener('click',function(){
                    var content=document.getElementById('prompt-content').value;
                    var tags=document.getElementById('prompt-tags').value.split(',').map(function(s){return s.trim();}).filter(function(s){return s!=='';});
                    apiCall('/api/prompts/validate',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({content:content,tags:tags})})
                        .then(function(r){return r.json();})
                        .then(function(d){
                            if(d.valid){
                                validationResult.classList.add('is-hidden');
                                validationResult.classList.remove('is-danger');
                                validationResult.textContent='';
                                var ok=document.createElement('div');
                                ok.className='notification is-success';
                                ok.textContent='Valid: all tokens and tags are OK.';
                                validationResult.parentNode.insertBefore(ok,validationResult);
                                setTimeout(function(){ok.remove();},2000);
                            }else{
                                var msgs=[];
                                if(d.unknownTokens&&d.unknownTokens.length)msgs.push('Unknown tokens: '+d.unknownTokens.join(', '));
                                if(d.invalidTags&&d.invalidTags.length)msgs.push('Invalid tags: '+d.invalidTags.join(', '));
                                validationResult.textContent=msgs.join(' | ');
                                validationResult.classList.remove('is-hidden');
                            }
                        });
                });

                // Composite children management
                var childRows=function(){return Array.prototype.slice.call(document.querySelectorAll('#child-list [data-child-id]'));};
                var childrenData=function(){
                    return childRows().map(function(row){
                        var entry=JSON.parse(row.getAttribute('data-child-entry'));
                        return {id:entry.id,title:entry.title,content:entry.content};
                    });
                };
                function renderPreview(){
                    var pre=document.getElementById('composite-preview');
                    if(!pre)return;
                    var parts=childrenData().map(function(c){return c.content;}).filter(function(c){return c!=='';});
                    pre.textContent=parts.join('\n---\n');
                }
                function populateSelect(){
                    var sel=document.getElementById('add-child-select');
                    if(!sel)return;
                    var existing=childrenData().map(function(c){return c.id;});
                    var library=window.__promptLibrary||[];
                    sel.innerHTML='';
                    library.filter(function(l){return existing.indexOf(l.id)===-1;}).forEach(function(l){
                        var opt=document.createElement('option');
                        opt.value=l.id;
                        opt.textContent=l.title;
                        sel.appendChild(opt);
                    });
                }
                var childrenDataRaw=document.getElementById('prompt-children-data');
                window.__promptChildren=childrenDataRaw?JSON.parse(childrenDataRaw.textContent):[];
                var libDataRaw=document.getElementById('prompt-library-data');
                window.__promptLibrary=libDataRaw?JSON.parse(libDataRaw.textContent):[];

                var addBtn=document.getElementById('add-child-btn');
                if(addBtn)addBtn.addEventListener('click',function(){
                    var sel=document.getElementById('add-child-select');
                    if(!sel.value)return;
                    var entry=window.__promptLibrary.filter(function(l){return l.id===sel.value;})[0];
                    if(!entry)return;
                    var row=document.createElement('div');
                    row.className='level is-mobile';
                    row.setAttribute('data-child-id',entry.id);
                    row.setAttribute('data-child-entry',JSON.stringify(entry));
                    row.innerHTML='<div class="level-left"><span class="icon has-text-grey"><i class="mdi mdi-grip-vertical"></i></span><span>'+entry.title+'</span></div>'+
                        '<div class="level-right"><div class="buttons are-small">'+
                        '<button class="button is-light" data-child-up title="Move up"><span class="icon is-small"><i class="mdi mdi-arrow-up"></i></span></button>'+
                        '<button class="button is-light" data-child-down title="Move down"><span class="icon is-small"><i class="mdi mdi-arrow-down"></i></span></button>'+
                        '<button class="button is-danger is-light" data-child-remove title="Remove"><span class="icon is-small"><i class="mdi mdi-close"></i></span></button>'+
                        '</div></div>';
                    document.getElementById('child-list').appendChild(row);
                    populateSelect();
                    renderPreview();
                });
                document.addEventListener('click',function(e){
                    var up=e.target.closest('[data-child-up]');
                    var down=e.target.closest('[data-child-down]');
                    var rem=e.target.closest('[data-child-remove]');
                    if(up||down||rem){
                        e.preventDefault();
                        var rows=childRows();
                        var row=up?up.closest('[data-child-id]'):down?down.closest('[data-child-id]'):rem.closest('[data-child-id]');
                        var idx=rows.indexOf(row);
                        if(rem){
                            row.remove();
                        }else if(up&&idx>0){
                            rows[idx-1].insertAdjacentElement('beforebegin',row);
                        }else if(down&&idx<rows.length-1){
                            rows[idx+1].insertAdjacentElement('afterend',row);
                        }
                        populateSelect();
                        renderPreview();
                    }
                });

                var saveBtn=document.getElementById('save-prompt-btn');
                if(saveBtn)saveBtn.addEventListener('click',function(){
                    var children=childrenData().map(function(c){return c.id;});
                    var data={
                        title:document.getElementById('prompt-title').value,
                        content:document.getElementById('prompt-content').value,
                        tags:document.getElementById('prompt-tags').value.split(',').map(function(s){return s.trim();}).filter(function(s){return s!=='';}),
                        is_shared:document.getElementById('share-toggle').checked,
                        children:children
                    };
                    apiCall('/api/prompts/'+promptId,{method:'PUT',headers:{'Content-Type':'application/json'},body:JSON.stringify(data)})
                        .then(function(r){if(r.ok)window.location.reload();});
                });

                var dupBtn=document.getElementById('duplicate-btn');
                if(dupBtn)dupBtn.addEventListener('click',function(){
                    apiCall('/api/prompts/'+promptId+'/duplicate',{method:'POST'})
                        .then(function(r){return r.json();})
                        .then(function(d){window.location.href='/webapp/prompts/'+d.id;});
                });
                var delBtn=document.getElementById('delete-prompt-btn');
                if(delBtn)delBtn.addEventListener('click',function(){
                    if(!confirm('Delete this prompt?'))return;
                    apiCall('/api/prompts/'+promptId,{method:'DELETE'})
                        .then(function(r){if(r.ok)window.location.href='/webapp/prompts';});
                });

                var designateBtn=document.getElementById('designate-btn');
                if(designateBtn)designateBtn.addEventListener('click',function(){
                    var scope=document.querySelector('input[name=designate-scope]:checked').value;
                    var projectId=scope==='project'?parseInt(document.getElementById('designate-project').value,10):null;
                    apiCall('/api/prompts/'+promptId+'/assignments',{method:'POST',headers:{'Content-Type':'application/json'},
                        body:JSON.stringify({agent_type:document.getElementById('designate-agent-type').value,scope_type:scope,project_id:projectId})})
                        .then(function(r){
                            if(r.ok){window.location.reload();return;}
                            r.json().then(function(d){
                                alert('Designation failed: '+(d.error||('HTTP '+r.status)));
                            }).catch(function(){alert('Designation failed (HTTP '+r.status+')');});
                        })
                        .catch(function(err){alert('Designation failed: '+err);});
                });
                document.addEventListener('click',function(e){
                    var del=e.target.closest('[data-assignment-delete]');
                    if(!del)return;
                    var id=del.closest('[data-assignment-id]').getAttribute('data-assignment-id');
                    apiCall('/api/prompts/assignments/'+id,{method:'DELETE'})
                        .then(function(r){if(r.ok)window.location.reload();});
                });

                // Project-scope select visibility
                function syncScope(){
                    var wrap=document.getElementById('project-select-wrap');
                    if(!wrap)return;
                    var scope=document.querySelector('input[name=designate-scope]:checked').value;
                    wrap.style.display=scope==='project'?'':'none';
                }
                document.querySelectorAll('input[name=designate-scope]').forEach(function(r){
                    r.addEventListener('change',syncScope);
                });
                syncScope();
                renderPreview();
                populateSelect();
            });"#}
        </script>
        <script id="prompt-children-data" type="application/json">{children_json}</script>
        <script id="prompt-library-data" type="application/json">{library_json}</script>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_prompt_detail_renders_editor_and_designate_box() {
        let owner = Uuid::new_v4();
        let mut p = make_prompt(
            "00000000-0000-0000-0000-000000000001",
            PromptKind::Snippet,
            "My Snippet",
        );
        p.owner_user_id = Some(owner);
        let html = leptos::view! {
            <PromptDetailPage
                prompt=p
                children=Vec::new()
                library=Vec::new()
                assignments=Vec::new()
                project_options=vec![(1, "ofm".into())]
                user_id=owner
            />
        }
        .to_html();
        assert!(html.contains("Prompt Builder"));
        assert!(html.contains("desktop-3d"));
        assert!(html.contains("Designate"));
        assert!(html.contains("Validate"));
        assert!(html.contains("Save"));
        assert!(html.contains("mdi-target"));
    }

    #[test]
    fn test_prompt_detail_static_hides_edit_controls() {
        let owner = Uuid::new_v4();
        let mut p = make_prompt(
            "00000000-0000-0000-0000-000000000002",
            PromptKind::Static,
            "Implementation",
        );
        p.is_static = true;
        p.owner_user_id = None;
        let html = leptos::view! {
            <PromptDetailPage
                prompt=p
                children=Vec::new()
                library=Vec::new()
                assignments=Vec::new()
                project_options=Vec::new()
                user_id=owner
            />
        }
        .to_html();
        assert!(html.contains("disabled"));
        assert!(
            !html.contains("mdi-content-save"),
            "static prompts must not render a Save button"
        );
        assert!(
            !html.contains("mdi-trash-can"),
            "static prompts must not render a Delete button"
        );
        assert!(
            !html.contains(r#"id="share-toggle""#),
            "static prompts must not render the share toggle"
        );
    }

    #[test]
    fn test_prompt_detail_composite_renders_children_and_preview() {
        let owner = Uuid::new_v4();
        let composite = make_prompt(
            "00000000-0000-0000-0000-000000000003",
            PromptKind::Composite,
            "Composite",
        );
        let child = make_prompt(
            "00000000-0000-0000-0000-000000000004",
            PromptKind::Snippet,
            "Child",
        );
        let html = leptos::view! {
            <PromptDetailPage
                prompt=composite
                children=vec![child]
                library=Vec::new()
                assignments=Vec::new()
                project_options=Vec::new()
                user_id=owner
            />
        }
        .to_html();
        assert!(html.contains("Composition"));
        assert!(html.contains("Child"));
        assert!(html.contains("composite-preview"));
        assert!(html.contains("add-child-select"));
    }
}
