use leptos::prelude::*;

#[component]
pub fn SsoLoginButton(label: &'static str) -> impl IntoView {
    view! {
        <button class="button is-small is-primary sso-login-btn">
            <span class="icon is-small"><i class="mdi mdi-login"></i></span>
            <span>{label}</span>
        </button>
        <script>
            {r#"document.addEventListener('DOMContentLoaded',function(){
                var returnTo = new URLSearchParams(window.location.search).get('return_to');

                function doLogin(btn) {
                    btn.disabled = true;
                    btn.textContent = 'Redirecting...';
                    var url = '/api/auth/login' + (returnTo ? '?return_to=' + encodeURIComponent(returnTo) : '');
                    fetch(url)
                        .then(function(r){return r.json();})
                        .then(function(data){
                            if(data.authorization_url){
                                window.location.href = data.authorization_url;
                            }
                        })
                        .catch(function(){
                            btn.disabled = false;
                            btn.textContent = 'Sign in with SSO';
                        });
                }

                if (returnTo) {
                    setTimeout(function() {
                        var btn = document.querySelector('.sso-login-btn');
                        if (!btn) return;
                        doLogin(btn);
                    }, 100);
                }

                document.querySelectorAll('.sso-login-btn').forEach(function(btn){
                    btn.addEventListener('click',function(){
                        doLogin(btn);
                    });
                });
            });"#}
        </script>
    }
}
