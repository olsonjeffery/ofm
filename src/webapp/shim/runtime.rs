pub fn global_runtime_script() -> String {
    r#"(function(){
    function loadIsland(el) {
        var url = el.dataset.islandUrl;
        if (!url) return;
        el.previousElementSibling.classList.add('island-loading');
        fetch(url)
            .then(function(r) { return r.ok ? r.text() : Promise.reject(r.status); })
            .then(function(html) {
                var wrapper = el.previousElementSibling;
                wrapper.insertAdjacentHTML('beforebegin', html);
                wrapper.remove();
                el.remove();
            })
            .catch(function(err) {
                var wrapper = el.previousElementSibling;
                wrapper.classList.remove('island-loading');
                wrapper.classList.add('island-error');
            });
    }

    document.addEventListener('DOMContentLoaded', function() {
        document.querySelectorAll('script[data-island-url]').forEach(loadIsland);
        handleCallbackPage();
    });

    document.addEventListener('click', function(ev) {
        var btn = ev.target.closest('[data-island-refresh]');
        if (!btn) return;
        var island = btn.closest('.island[data-island]');
        if (!island) return;
        var script = island.nextElementSibling;
        if (script && script.hasAttribute('data-island-url')) {
            loadIsland(script);
        }
    });

    // SSO login button click handler
    document.addEventListener('click', function(ev) {
        var btn = ev.target.closest('#sso-login-btn');
        if (!btn) return;
        btn.disabled = true;
        btn.textContent = 'Redirecting...';
        fetch('/api/auth/login')
            .then(function(r) { return r.json(); })
            .then(function(data) {
                if (data.authorization_url) {
                    window.location.href = data.authorization_url;
                }
            })
            .catch(function() {
                btn.disabled = false;
                btn.textContent = 'Sign in with SSO';
            });
    });

    // API call helper with Bearer token and auto-refresh on 401
    window.__ACCESS_TOKEN__ = window.__ACCESS_TOKEN__ || null;

    function apiCall(url, options) {
        options = options || {};
        options.headers = options.headers || {};
        if (window.__ACCESS_TOKEN__) {
            options.headers['Authorization'] = 'Bearer ' + window.__ACCESS_TOKEN__;
        }
        return fetch(url, options).then(function(r) {
            if (r.status === 401) {
                return fetch('/api/auth/refresh', { method: 'POST', credentials: 'same-origin' })
                    .then(function(refreshR) {
                        if (!refreshR.ok) { window.location.href = '/webapp/login'; throw new Error('Session expired'); }
                        return refreshR.json();
                    })
                    .then(function(data) {
                        window.__ACCESS_TOKEN__ = data.access_token;
                        options.headers['Authorization'] = 'Bearer ' + window.__ACCESS_TOKEN__;
                        return fetch(url, options);
                    });
            }
            return r;
        });
    }

    // Logout form handler
    document.addEventListener('submit', function(ev) {
        var form = ev.target.closest('#logout-form');
        if (!form) return;
        ev.preventDefault();
        fetch(form.action, { method: 'POST', credentials: 'same-origin' })
            .then(function() { window.location.href = '/webapp/login'; })
            .catch(function() { window.location.href = '/webapp/login'; });
    });

    // Onboarding form submission handler
    document.addEventListener('submit', function(ev) {
        var form = ev.target.closest('#onboarding-form');
        if (!form) return;
        ev.preventDefault();
        var data = {
            git_name: form.git_name.value,
            git_email: form.git_email.value,
            is_technical: form.is_technical.value === 'true'
        };
        var btn = form.querySelector('button[type="submit"]');
        btn.disabled = true;
        btn.textContent = 'Saving...';
        apiCall('/api/auth/onboarding', {
            method: 'PATCH',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify(data)
        }).then(function(r) {
            if (r.ok) { window.location.href = '/webapp/'; }
            else { btn.disabled = false; btn.textContent = 'Complete Setup'; }
        }).catch(function() {
            btn.disabled = false; btn.textContent = 'Complete Setup';
        });
    });

    // Callback page handler: check if onboarding is needed
    function handleCallbackPage() {
        var user = window.__USER__;
        if (!user) return;
        if (user.has_completed_onboarding) {
            window.location.href = '/webapp/';
        } else {
            var root = document.getElementById('callback-root');
            if (root && window.__ONBOARDING_HTML__) {
                root.innerHTML = window.__ONBOARDING_HTML__;
                root.className = '';
            }
        }
    }
})();"#
        .to_string()
}
