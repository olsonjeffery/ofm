use leptos::prelude::*;

#[component]
pub fn OnboardingForm(git_name: String, git_email: String, is_technical: bool) -> impl IntoView {
    view! {
        <section class="section">
            <div class="columns is-centered">
                <div class="column is-half">
                    <div class="box">
                        <h2 class="title is-4">"User Onboarding Config"</h2>
                        <p>"Set up your git identity and preferences."</p>
                        <form id="onboarding-form">
                            <div class="field">
                                <label class="label">"Git Name"</label>
                                <div class="control has-icons-left">
                                    <input type="text" name="git_name" required placeholder="e.g. Jane Doe" class="input" value=git_name />
                                    <span class="icon is-left is-small"><i class="mdi mdi-account"></i></span>
                                </div>
                            </div>
                            <div class="field">
                                <label class="label">"Git Email"</label>
                                <div class="control has-icons-left">
                                    <input type="email" name="git_email" required placeholder="e.g. jane@example.com" class="input" value=git_email />
                                    <span class="icon is-left is-small"><i class="mdi mdi-email"></i></span>
                                </div>
                            </div>
                            <div class="field">
                                <label class="label">"Role"</label>
                                <div class="control">
                                    <label class="radio">
                                        <input type="radio" name="is_technical" value="true" checked=is_technical />
                                        <span class="icon is-small"><i class="mdi mdi-code-tags"></i></span>
                                        " Technical"
                                    </label>
                                    <label class="radio">
                                        <input type="radio" name="is_technical" value="false" checked=!is_technical />
                                        <span class="icon is-small"><i class="mdi mdi-account-outline"></i></span>
                                        " Non-technical"
                                    </label>
                                </div>
                            </div>
                            <button type="submit" class="button is-primary">
                                <span class="icon is-small"><i class="mdi mdi-account-check"></i></span>
                                <span>"Save"</span>
                            </button>
                        </form>
                    </div>
                </div>
            </div>
        </section>
        <script>
            {r#"document.addEventListener('DOMContentLoaded',function(){
                var form=document.getElementById('onboarding-form');
                if(!form)return;
                form.addEventListener('submit',function(ev){
                    ev.preventDefault();
                    var data={
                        git_name: form.git_name.value,
                        git_email: form.git_email.value,
                        is_technical: form.is_technical.value==='true'
                    };
                    var btn=form.querySelector('button[type="submit"]');
                    btn.disabled=true;
                    btn.textContent='Saving...';
                    apiCall('/api/auth/onboarding',{
                        method:'PATCH',
                        headers:{'Content-Type':'application/json'},
                        body:JSON.stringify(data)
                    }).then(function(r){
                        if(r.ok){window.location.href='/webapp/';}
                        else{btn.disabled=false;btn.innerHTML='<span class=\"icon is-small\"><i class=\"mdi mdi-account-check\"></i></span><span>Save</span>';}
                    }).catch(function(){
                        btn.disabled=false;btn.innerHTML='<span class=\"icon is-small\"><i class=\"mdi mdi-account-check\"></i></span><span>Save</span>';
                    });
                });
            });"#}
        </script>
    }
}

pub fn render_onboarding_form(git_name: String, git_email: String, is_technical: bool) -> String {
    crate::webapp::shim::render_component(move || view! { <OnboardingForm git_name git_email is_technical /> })
}
