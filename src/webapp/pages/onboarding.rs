use leptos::prelude::*;

#[component]
pub fn OnboardingForm() -> impl IntoView {
    view! {
        <section class="section">
            <div class="columns is-centered">
                <div class="column is-half">
                    <div class="box">
                        <h2>"Complete Your Profile"</h2>
                        <p>"Set up your git identity to get started."</p>
                        <form id="onboarding-form">
                            <div class="field">
                                <label class="label">"Git Name"</label>
                                <div class="control">
                                    <input type="text" name="git_name" required placeholder="e.g. Jane Doe" class="input"/>
                                </div>
                            </div>
                            <div class="field">
                                <label class="label">"Git Email"</label>
                                <div class="control">
                                    <input type="email" name="git_email" required placeholder="e.g. jane@example.com" class="input"/>
                                </div>
                            </div>
                            <div class="field">
                                <div class="control">
                                    <label class="radio">
                                        <input type="radio" name="is_technical" value="true"/>
                                        " Technical"
                                    </label>
                                    <label class="radio">
                                        <input type="radio" name="is_technical" value="false" checked/>
                                        " Non-technical"
                                    </label>
                                </div>
                            </div>
                            <button type="submit" class="button is-primary">"Complete Setup"</button>
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
                        else{btn.disabled=false;btn.textContent='Complete Setup';}
                    }).catch(function(){
                        btn.disabled=false;btn.textContent='Complete Setup';
                    });
                });
            });"#}
        </script>
    }
}

pub fn render_onboarding_form() -> String {
    crate::webapp::shim::render_component(|| view! { <OnboardingForm /> })
}
