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

    window.__ACCESS_TOKEN__ = null;

    function decodeJwtPayload(token) {
        try {
            var parts = token.split('.');
            if (parts.length !== 3) return null;
            return JSON.parse(atob(parts[1].replace(/-/g, '+').replace(/_/g, '/')));
        } catch(e) { return null; }
    }

    function isTokenExpired(token, graceSeconds) {
        if (!token) return true;
        var payload = decodeJwtPayload(token);
        if (!payload || !payload.exp) return true;
        return (Date.now() / 1000) + graceSeconds >= payload.exp;
    }

    function refreshAccessToken() {
        return fetch('/api/auth/refresh', { method: 'POST', credentials: 'same-origin' })
            .then(function(r) {
                if (!r.ok) { window.__ACCESS_TOKEN__ = null; return null; }
                return r.json();
            })
            .then(function(data) {
                if (data && data.access_token) {
                    window.__ACCESS_TOKEN__ = data.access_token;
                    return data.access_token;
                }
                window.__ACCESS_TOKEN__ = null;
                return null;
            })
            .catch(function() {
                window.__ACCESS_TOKEN__ = null;
                return null;
            });
    }

    function addAuthHeader(options) {
        options.headers = options.headers || {};
        if (window.__ACCESS_TOKEN__ && !isTokenExpired(window.__ACCESS_TOKEN__, 60)) {
            options.headers['Authorization'] = 'Bearer ' + window.__ACCESS_TOKEN__;
        }
        return options;
    }

    window.apiCall = function(url, options) {
        options = options || {};

        function doFetch() {
            addAuthHeader(options);
            return fetch(url, options).then(function(r) {
                if (r.status === 401) {
                    return refreshAccessToken().then(function() {
                        if (!window.__ACCESS_TOKEN__) {
                            window.location.href = '/webapp/login';
                            throw new Error('Session expired');
                        }
                        addAuthHeader(options);
                        return fetch(url, options);
                    });
                }
                return r;
            });
        }

        if (isTokenExpired(window.__ACCESS_TOKEN__, 120)) {
            return refreshAccessToken().then(doFetch);
        }
        return doFetch();
    };

    fetch('/api/auth/refresh', { method: 'POST', credentials: 'same-origin' })
        .then(function(r) { return r.ok ? r.json() : null; })
        .then(function(data) {
            if (data && data.access_token) {
                window.__ACCESS_TOKEN__ = data.access_token;
            }
        })
        .catch(function() {});
})();"#
        .to_string()
}
