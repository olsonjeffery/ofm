use leptos::prelude::*;
use std::sync::LazyLock;
use std::time::Instant;

static START: LazyLock<Instant> = LazyLock::new(Instant::now);

#[component]
pub fn UptimeIsland() -> impl IntoView {
    let elapsed = format!("{:?}", START.elapsed());
    view! {
        <div data-island="uptime">
            <h2>"Server Uptime"</h2>
            <p class="uptime-value">{elapsed}</p>
            <button data-island-refresh>"Update"</button>
        </div>
    }
}

pub fn render_uptime() -> String {
    let inner = crate::webapp::shim::render_component(|| view! { <UptimeIsland /> });
    crate::webapp::shim::wrap_island("uptime", "/webapp/islands/uptime", "", inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_uptime_contains_server_uptime() {
        let html = render_uptime();
        assert!(html.contains("Server Uptime"));
        assert!(html.contains(r#"data-island="uptime""#));
    }
}
