//! System Status & Health page (freestanding; visible to any authenticated
//! user from the navbar agent dropdown).
//!
//! Renders the markdown report via `<MarkdownViewer>` (unicode status icons
//! pass through ammonia sanitization) plus a "Live Data" card grid driven by
//! `/api/system/status`. Refresh JS subscribes to the System topic and
//! re-fetches on `system_status` events, with a 30s `setInterval` safety net.

use html_escape::encode_text;
use leptos::prelude::*;

pub fn render(markdown: String, json: serde_json::Value, can_use: bool) -> String {
    let entries = json["entries"].as_array().cloned().unwrap_or_default();
    let running = json["running_services"].as_i64().unwrap_or(0);
    let generated_at = json["generated_at"].as_str().unwrap_or("").to_string();

    let mut cards = String::new();
    for e in &entries {
        let resource = encode_text(e["resource"].as_str().unwrap_or(""));
        let category = e["category"].as_str().unwrap_or("");
        let status = e["status"].as_str().unwrap_or("unknown");
        let detail = encode_text(e["detail"].as_str().unwrap_or(""));
        let created_at = e["created_at"].as_str().unwrap_or("");
        let meta = &e["metadata"];
        let version = meta.get("version").and_then(|v| v.as_str()).unwrap_or("—");
        let path = meta.get("path").and_then(|v| v.as_str()).unwrap_or("—");
        let pid = meta
            .get("pid")
            .and_then(|v| v.as_i64())
            .map(|p| p.to_string())
            .unwrap_or_else(|| "—".into());
        let ram = meta
            .get("ram_kb")
            .and_then(|v| v.as_i64())
            .map(|k| format!("{k} KB"))
            .unwrap_or_else(|| "—".into());
        let last = meta
            .get("last_interaction")
            .and_then(|v| v.as_str())
            .unwrap_or("—");

        let (status_class, icon) = status_badge(status);
        cards.push_str(&format!(
            r#"
<div class="card" data-health-resource="{resource}" data-health-category="{category}" data-health-status="{status}">
    <div class="card-content">
        <div class="level is-mobile">
            <div class="level-left">
                <span class="tag {status_class}">
                    <span class="icon is-small"><i class="mdi mdi-{icon}"></i></span>
                    <span>{status}</span>
                </span>
                <strong class="is-size-7" style="margin-left: 0.5rem">{resource}</strong>
            </div>
            <div class="level-right">
                <span class="has-text-grey is-size-7" data-utc="{created_at}">{created_at}</span>
            </div>
        </div>
        <p class="is-size-7 has-text-grey">{detail}</p>
        <div class="tags is-size-7" style="margin-top: 0.5rem">
            <span class="tag is-light">version: {version}</span>
            <span class="tag is-light">path: {path}</span>
            <span class="tag is-light">pid: {pid}</span>
            <span class="tag is-light">ram: {ram}</span>
            <span class="tag is-light">last interaction: {last}</span>
        </div>
    </div>
</div>"#
        ));
    }

    let script = r#"(function(){
    var countEl = document.getElementById('running-services-count');
    var cardsEl = document.getElementById('system-status-cards');

    function statusBadge(status) {
        if (status === 'ok') return ['is-success', 'check-circle-outline'];
        if (status === 'warn') return ['is-warning', 'alert-outline'];
        if (status === 'missing') return ['is-dark', 'minus-circle-outline'];
        if (status === 'error') return ['is-danger', 'close-circle-outline'];
        return ['is-light', 'help-circle-outline'];
    }

    function esc(s) {
        var d = document.createElement('div');
        d.appendChild(document.createTextNode(s == null ? '' : String(s)));
        return d.innerHTML;
    }

    function renderCards(entries) {
        if (!cardsEl) return;
        var html = '';
        for (var i = 0; i < entries.length; i++) {
            var e = entries[i];
            var meta = e.metadata || {};
            var badge = statusBadge(e.status);
            var pid = meta.pid != null ? meta.pid : '—';
            var ram = meta.ram_kb != null ? meta.ram_kb + ' KB' : '—';
            var last = meta.last_interaction != null ? meta.last_interaction : '—';
            var created = e.created_at || '';
            html += '<div class="card" data-health-resource="' + esc(e.resource) + '" data-health-category="' + esc(e.category) + '" data-health-status="' + esc(e.status) + '">' +
                '<div class="card-content">' +
                    '<div class="level is-mobile">' +
                        '<div class="level-left">' +
                            '<span class="tag ' + badge[0] + '"><span class="icon is-small"><i class="mdi mdi-' + badge[1] + '"></i></span><span>' + esc(e.status) + '</span></span>' +
                            '<strong class="is-size-7" style="margin-left: 0.5rem">' + esc(e.resource) + '</strong>' +
                        '</div>' +
                        '<div class="level-right"><span class="has-text-grey is-size-7" data-utc="' + esc(created) + '">' + esc(created) + '</span></div>' +
                    '</div>' +
                    '<p class="is-size-7 has-text-grey">' + esc(e.detail) + '</p>' +
                    '<div class="tags is-size-7" style="margin-top: 0.5rem">' +
                        '<span class="tag is-light">version: ' + esc(meta.version || '—') + '</span>' +
                        '<span class="tag is-light">path: ' + esc(meta.path || '—') + '</span>' +
                        '<span class="tag is-light">pid: ' + esc(pid) + '</span>' +
                        '<span class="tag is-light">ram: ' + esc(ram) + '</span>' +
                        '<span class="tag is-light">last interaction: ' + esc(last) + '</span>' +
                    '</div>' +
                '</div>' +
            '</div>';
        }
        cardsEl.innerHTML = html;
        if (window.OfmTime) window.OfmTime.apply(cardsEl);
    }

    function refresh() {
        window.apiCall('/api/system/status')
            .then(function(r) { return r.ok ? r.json() : Promise.reject(r.status); })
            .then(function(data) {
                if (countEl) countEl.textContent = data.running_services != null ? data.running_services : '—';
                renderCards(data.entries || []);
            })
            .catch(function() {});
    }

    function handleSystemStatus(msg) {
        if (msg.event_type === 'system_status') refresh();
    }

    var subscribed = false;
    function subscribe() {
        if (subscribed || !window.OfmWS) return;
        subscribed = true;
        window.OfmWS.subscribe({ kind: 'system', id: 0 }, handleSystemStatus);
    }

    if (window.OfmWS && window.OfmWS.status === 'connected') subscribe();
    document.addEventListener('ws-status-changed', function(ev) {
        if (ev.detail.status === 'connected') subscribe();
    });
    setInterval(function() {
        if (window.OfmWS && window.OfmWS.status === 'connected') refresh();
    }, 30000);
})();"#;

    leptos::view! {
        <div class="box" id="system-status-page">
            <h1 class="title is-4">
                <span class="icon"><i class="mdi mdi-heart-pulse"></i></span>
                <span>"System Status & Health"</span>
            </h1>
            <p class="heading">
                "Running services: "
                <strong id="running-services-count">{running}</strong>
                " · generated "
                <span data-utc={generated_at.clone()} data-utc-format="datetime">{generated_at.clone()}</span>
            </p>
            <hr/>
            <div id="system-status-markdown">
                <crate::webapp::components::markdown_viewer::MarkdownViewer content=markdown />
            </div>
            <h2 class="title is-5" style="margin-top: 1.5rem">
                <span class="icon"><i class="mdi mdi-server"></i></span>
                <span>"Live Data"</span>
            </h2>
            <div id="system-status-cards" class="grid" style="--bulma-grid-column-count: 2" inner_html=cards></div>
            {if can_use {
                ().into_any()
            } else {
                view! { <p class="help">"Available to agents with the `system-status` capability."</p> }.into_any()
            }}
        </div>
        <script>{script}</script>
    }
    .to_html()
}

/// `(bulma tag class, mdi icon name)` for a status string.
pub fn status_badge(status: &str) -> (&'static str, &'static str) {
    match status {
        "ok" => ("is-success", "check-circle-outline"),
        "warn" => ("is-warning", "alert-outline"),
        "missing" => ("is-dark", "minus-circle-outline"),
        "error" => ("is-danger", "close-circle-outline"),
        _ => ("is-light", "help-circle-outline"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_badge_mapping() {
        assert_eq!(status_badge("ok"), ("is-success", "check-circle-outline"));
        assert_eq!(status_badge("warn"), ("is-warning", "alert-outline"));
        assert_eq!(status_badge("missing"), ("is-dark", "minus-circle-outline"));
        assert_eq!(status_badge("error"), ("is-danger", "close-circle-outline"));
        assert_eq!(status_badge("bogus"), ("is-light", "help-circle-outline"));
    }

    #[test]
    fn test_render_contains_sections_and_icons() {
        let json = serde_json::json!({
            "generated_at": "2026-01-01 00:00:00",
            "running_services": 2,
            "entries": [
                {
                    "category": "live",
                    "resource": "live:hiqlite",
                    "status": "ok",
                    "detail": "hiqlite cluster healthy=true",
                    "metadata": {"pid": 5, "ram_kb": 10},
                    "created_at": "2026-01-01 00:00:00"
                }
            ]
        });
        let md = "## Dependency Check\n\n- ✔ **bin:git** — ok — git found at /usr/bin/git";
        let html = render(md.to_string(), json, true);
        assert!(html.contains("System Status &amp; Health"));
        assert!(html.contains("Live Data"));
        assert!(html.contains("running-services-count"));
        assert!(html.contains("live:hiqlite"));
        assert!(html.contains("data-utc="));
        assert!(
            html.contains("<h2>Dependency Check</h2>"),
            "markdown should be rendered to HTML by MarkdownViewer"
        );
        assert!(
            !html.contains("system-status capability"),
            "capability note hidden when allowed"
        );
    }

    #[test]
    fn test_render_capability_note_when_not_allowed() {
        let json = serde_json::json!({"generated_at": "x", "running_services": 0, "entries": []});
        let html = render(String::new(), json, false);
        assert!(
            html.contains("Available to agents with the `system-status` capability"),
            "non-capable users see the muted note"
        );
    }
}
