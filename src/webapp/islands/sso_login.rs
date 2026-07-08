use leptos::prelude::*;

#[component]
pub fn SsoLoginButton(label: &'static str) -> impl IntoView {
    view! {
        <button class="button is-primary sso-login-btn">
            <span class="icon is-small"><i class="mdi mdi-login"></i></span>
            <span>{label}</span>
        </button>
        <script>
            {r#"document.addEventListener('DOMContentLoaded',function(){
                document.querySelectorAll('.sso-login-btn').forEach(function(btn){
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
                });
            });"#}
        </script>
    }
}
