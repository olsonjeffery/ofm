use leptos::prelude::*;

#[component]
pub fn LoginPage() -> impl IntoView {
    view! {
        <section class="section">
            <div class="columns is-centered">
                <div class="column is-half">
                    <div class="box has-text-centered">
                        <h2>"Sign in to omprint"</h2>
                        <p>"Authenticate with your SSO provider to continue."</p>
                        <button id="sso-login-btn" class="button is-primary">
                            <span class="icon is-small"><i class="mdi mdi-login"></i></span>
                            "Sign in with SSO"
                        </button>
                    </div>
                </div>
            </div>
        </section>
        <script>
            {r#"document.addEventListener('DOMContentLoaded',function(){
                var btn=document.getElementById('sso-login-btn');
                if(!btn)return;
                btn.addEventListener('click',function(){
                    btn.disabled=true;
                    btn.textContent='Redirecting...';
                    fetch('/api/auth/login')
                        .then(function(r){return r.json();})
                        .then(function(data){
                            if(data.authorization_url){
                                window.location.href=data.authorization_url;
                            }
                        })
                        .catch(function(){
                            btn.disabled=false;
                            btn.textContent='Sign in with SSO';
                        });
                });
            });"#}
        </script>
    }
}

pub fn render_login_page() -> String {
    crate::webapp::shim::render_component(|| view! { <LoginPage /> })
}
