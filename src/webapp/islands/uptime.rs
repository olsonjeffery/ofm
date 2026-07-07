use leptos::prelude::*;
use std::sync::LazyLock;
use std::time::Instant;

static START: LazyLock<Instant> = LazyLock::new(Instant::now);

#[component]
pub fn UptimeIsland() -> impl IntoView {
    let elapsed = format!("{:?}", START.elapsed());
    view! {
        <div data-island="uptime">
            <h2 class="title is-5">"Server Uptime"</h2>
            <p class="uptime-value has-text-weight-bold is-family-monospace">{elapsed}</p>
            <button class="button is-small is-light" data-island-refresh>
                <span class="icon is-small"><i class="mdi mdi-refresh"></i></span>
                <span>"Update"</span>
            </button>
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
