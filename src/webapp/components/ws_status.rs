use leptos::prelude::*;

#[component]
pub fn WsStatus() -> impl IntoView {
    let status_id = "ws-status-indicator";

    view! {
        <span id={status_id} class="navbar-item ws-status" data-status="disconnected">
            <span class="icon is-small">
                <i class="mdi mdi-wifi"></i>
            </span>
            <span class="ws-status-label">"Disconnected"</span>
        </span>
        <script>
            {format!(
                r#"document.addEventListener('DOMContentLoaded',function(){{
                    var el=document.getElementById('{id}');
                    if(!el)return;
                    var dot=el.querySelector('.mdi');
                    var label=el.querySelector('.ws-status-label');
                    function update(status){{
                        el.dataset.status=status;
                        if(status==='connected'){{
                            dot.className='mdi mdi-wifi';
                            label.textContent='Live';
                        }}else if(status==='connecting'){{
                            dot.className='mdi mdi-wifi-off';
                            label.textContent='Connecting...';
                        }}else{{
                            dot.className='mdi mdi-wifi-off';
                            label.textContent='Disconnected';
                        }}
                    }}
                    if(window.OmprintWS&&window.OmprintWS.status){{
                        update(window.OmprintWS.status);
                    }}
                    document.addEventListener('ws-status-changed',function(ev){{
                        update(ev.detail.status);
                    }});
                }});"#,
                id = status_id
            )}
        </script>
        <style>
            {r#".ws-status[data-status="connected"] { color: #48c774; }
            .ws-status[data-status="connecting"] { color: #ffdd57; }
            .ws-status[data-status="disconnected"] { color: #f14668; }"#}
        </style>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ws_status_renders_disconnected_initially() {
        let html = leptos::view! { <WsStatus /> }.to_html();
        assert!(html.contains("Disconnected"));
        assert!(html.contains("mdi-wifi-off"));
        assert!(html.contains("ws-status"));
    }

    #[test]
    fn test_ws_status_contains_status_id() {
        let html = leptos::view! { <WsStatus /> }.to_html();
        assert!(html.contains("ws-status-indicator"));
    }

    #[test]
    fn test_ws_status_has_js_listener() {
        let html = leptos::view! { <WsStatus /> }.to_html();
        assert!(html.contains("ws-status-changed"));
        assert!(html.contains("OmprintWS"));
    }

    #[test]
    fn test_ws_status_contains_css_for_all_states() {
        let html = leptos::view! { <WsStatus /> }.to_html();
        assert!(html.contains("#48c774"));
        assert!(html.contains("#ffdd57"));
        assert!(html.contains("#f14668"));
    }
}
