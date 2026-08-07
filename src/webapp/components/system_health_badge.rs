//! Navbar "running services" badge. Shows the count of live (`ok`) services
//! from `/api/system/status`, driven by the System-topic `system_status`
//! broadcasts with a 30s poll safety net. Mirrors the `agent_dropdown` WS
//! pattern.

use leptos::prelude::*;

#[component]
pub fn SystemHealthBadge() -> impl IntoView {
    view! {
        <span class="navbar-item" id="system-health-badge" title="Running services">
            <span class="icon is-small"><i class="mdi mdi-heart-pulse"></i></span>
            <span id="system-health-count">"–"</span>
        </span>
        <script>
            {r#"(function(){
                var el = document.getElementById('system-health-count');
                function update(count) {
                    if (el) el.textContent = count;
                }
                function fetchStatus() {
                    window.apiCall('/api/system/status')
                        .then(function(r) { return r.ok ? r.json() : Promise.reject(r.status); })
                        .then(function(data) {
                            update(data.running_services != null ? data.running_services : '–');
                        })
                        .catch(function() {});
                }
                var subscribed = false;
                function subscribe() {
                    if (subscribed || !window.OfmWS) return;
                    subscribed = true;
                    window.OfmWS.subscribe({ kind: 'system', id: 0 }, function(msg) {
                        if (msg.event_type === 'system_status') {
                            if (msg.payload && msg.payload.running_services != null) {
                                update(msg.payload.running_services);
                            } else {
                                fetchStatus();
                            }
                        }
                    });
                }
                if (window.OfmWS && window.OfmWS.status === 'connected') { subscribe(); fetchStatus(); }
                document.addEventListener('ws-status-changed', function(ev) {
                    if (ev.detail.status === 'connected') { subscribe(); fetchStatus(); }
                });
                setInterval(function() {
                    if (window.OfmWS && window.OfmWS.status === 'connected') fetchStatus();
                }, 30000);
            })();"#}
        </script>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_badge_markup_present() {
        let html = leptos::view! { <SystemHealthBadge /> }.to_html();
        assert!(html.contains("system-health-badge"));
        assert!(html.contains("system-health-count"));
        assert!(html.contains("mdi-heart-pulse"));
        assert!(html.contains("/api/system/status"));
        assert!(html.contains("system_status"));
        assert!(html.contains("{ kind: 'system', id: 0 }"));
    }
}
