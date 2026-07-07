use leptos::prelude::*;

#[component]
pub fn OnboardingForm() -> impl IntoView {
    view! {
        <div class="onboarding-card">
            <h2>"Complete Your Profile"</h2>
            <p>"Set up your git identity to get started."</p>
            <form id="onboarding-form">
                <label>
                    "Git Name"
                    <input type="text" name="git_name" required placeholder="e.g. Jane Doe"/>
                </label>
                <label>
                    "Git Email"
                    <input type="email" name="git_email" required placeholder="e.g. jane@example.com"/>
                </label>
                <fieldset>
                    <legend>"Role"</legend>
                    <label>
                        <input type="radio" name="is_technical" value="true"/>
                        " Technical"
                    </label>
                    <label>
                        <input type="radio" name="is_technical" value="false" checked/>
                        " Non-technical"
                    </label>
                </fieldset>
                <button type="submit" class="btn btn-primary">"Complete Setup"</button>
            </form>
        </div>
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
