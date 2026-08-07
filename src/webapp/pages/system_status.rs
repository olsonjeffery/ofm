//! System Status & Health page (freestanding; visible to any authenticated
//! user from the navbar agent dropdown).
//!
//! Renders the markdown report via `<MarkdownViewer>` (unicode status icons
//! pass through ammonia sanitization) plus a "Live Data" card grid driven by
//! `/api/system/status`. Refresh JS subscribes to the System topic and
//! re-fetches on `system_status` events, with a 30s `setInterval` safety net.

use leptos::prelude::*;

pub fn render(markdown: String, json: serde_json::Value, can_use: bool) -> String {
    let running = json["running_services"].as_i64().unwrap_or(0);
    let generated_at = json["generated_at"].as_str().unwrap_or("").to_string();

    if !can_use {
        return leptos::view! {
            <div class="notification is-danger">
                "You aren't authorized to use this page"
            </div>
        }
        .to_html();
    };

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
        </div>
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
