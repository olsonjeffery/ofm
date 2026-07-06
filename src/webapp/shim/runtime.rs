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

    window.__ACCESS_TOKEN__ = window.__ACCESS_TOKEN__ || null;

    window.apiCall = function apiCall(url, options) {
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
})();"#
        .to_string()
}
