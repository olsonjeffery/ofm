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

    window.OmprintWS = {
        status: 'disconnected',
        _ws: null,
        _subscriptions: {},
        _statusCallbacks: [],
        _reconnectDelay: 1000,
        _pingInterval: null,
        _pongTimeout: null,
        _intentionalClose: false,

        connect: function() {
            if (this._ws && (this._ws.readyState === WebSocket.OPEN || this._ws.readyState === WebSocket.CONNECTING)) return;
            this._setStatus('connecting');
            this._intentionalClose = false;
            var protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
            var url = protocol + '//' + location.host + '/ws';
            this._ws = new WebSocket(url);
            var self = this;
            this._ws.onopen = function() {
                self._setStatus('connected');
                self._reconnectDelay = 1000;
                self._startPing();
                self._resubscribe();
            };
            this._ws.onclose = function(ev) {
                self._stopPing();
                if (!self._intentionalClose) {
                    self._setStatus('disconnected');
                    self._scheduleReconnect();
                }
            };
            this._ws.onerror = function() {};
            this._ws.onmessage = function(ev) { self._handleMessage(ev.data); };
        },

        disconnect: function() {
            this._intentionalClose = true;
            this._stopPing();
            if (this._ws) { this._ws.close(); this._ws = null; }
            this._setStatus('disconnected');
        },

        send: function(msg) {
            if (this._ws && this._ws.readyState === WebSocket.OPEN) {
                this._ws.send(JSON.stringify(msg));
            }
        },

        subscribe: function(topic, callback) {
            var key = topic.kind + ':' + topic.id;
            if (!this._subscriptions[key]) {
                this._subscriptions[key] = { topic: topic, callbacks: [] };
            }
            this._subscriptions[key].callbacks.push(callback);
            if (this.status === 'connected') {
                this.send({ type: 'subscribe', topics: [topic] });
            }
        },

        unsubscribe: function(topic, callback) {
            var key = topic.kind + ':' + topic.id;
            if (!this._subscriptions[key]) return;
            var cbs = this._subscriptions[key].callbacks;
            var idx = cbs.indexOf(callback);
            if (idx !== -1) cbs.splice(idx, 1);
            if (cbs.length === 0) {
                delete this._subscriptions[key];
                this.send({ type: 'unsubscribe', topics: [topic] });
            }
        },

        onStatusChange: function(callback) {
            this._statusCallbacks.push(callback);
        },

        _setStatus: function(s) {
            this.status = s;
            document.dispatchEvent(new CustomEvent('ws-status-changed', { detail: { status: s } }));
            for (var i = 0; i < this._statusCallbacks.length; i++) {
                this._statusCallbacks[i](s);
            }
        },

        _resubscribe: function() {
            var topics = [];
            var since = localStorage.getItem('ws_last_event_time');
            for (var key in this._subscriptions) {
                topics.push(this._subscriptions[key].topic);
            }
            if (topics.length > 0) {
                var msg = { type: 'subscribe', topics: topics };
                if (since) msg.since = since;
                this.send(msg);
            }
        },

        _handleMessage: function(data) {
            var msg;
            try { msg = JSON.parse(data); } catch(e) { return; }
            switch (msg.type) {
                case 'event':
                    if (msg.timestamp) localStorage.setItem('ws_last_event_time', msg.timestamp);
                    var key = msg.topic.kind + ':' + msg.topic.id;
                    var sub = this._subscriptions[key];
                    if (sub) {
                        for (var i = 0; i < sub.callbacks.length; i++) sub.callbacks[i](msg);
                    }
                    break;
                case 'events_replay':
                    if (msg.timestamp) localStorage.setItem('ws_last_event_time', msg.timestamp);
                    for (var i = 0; i < msg.events.length; i++) {
                        var ev = msg.events[i];
                        var key = ev.topic.kind + ':' + ev.topic.id;
                        var sub = this._subscriptions[key];
                        if (sub) {
                            for (var j = 0; j < sub.callbacks.length; j++) sub.callbacks[j](ev);
                        }
                    }
                    break;
                case 'pong':
                    if (this._pongTimeout) { clearTimeout(this._pongTimeout); this._pongTimeout = null; }
                    break;
            }
        },

        _startPing: function() {
            var self = this;
            this._stopPing();
            this._pingInterval = setInterval(function() {
                self.send({ type: 'ping' });
                self._pongTimeout = setTimeout(function() {
                    if (self._ws) self._ws.close();
                }, 10000);
            }, 30000);
        },

        _stopPing: function() {
            if (this._pingInterval) { clearInterval(this._pingInterval); this._pingInterval = null; }
            if (this._pongTimeout) { clearTimeout(this._pongTimeout); this._pongTimeout = null; }
        },

        _scheduleReconnect: function() {
            var self = this;
            setTimeout(function() { self._setStatus('connecting'); self.connect(); }, this._reconnectDelay);
            this._reconnectDelay = Math.min(this._reconnectDelay * 2, 30000);
        }
    };

    document.addEventListener('DOMContentLoaded', function() { window.OmprintWS.connect(); });

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
