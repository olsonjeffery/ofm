use leptos::prelude::*;

#[component]
pub fn InfoCard(title: String, body: String) -> impl IntoView {
    view! {
        <div data-island="infocard">
            <h3 class="title is-5">{title}</h3>
            <p class="subtitle">{body}</p>
        </div>
    }
}

pub fn render_infocard(title: &str, body: &str) -> String {
    let inner = {
        let t = title.to_string();
        let b = body.to_string();
        crate::webapp::shim::render_component(move || {
            view! { <InfoCard title=t body=b /> }
        })
    };
    let qs = format!("title={}&body={}", urlencoding(title), urlencoding(body));
    crate::webapp::shim::wrap_island("infocard", "/webapp/islands/infocard", &qs, inner)
}

fn urlencoding(s: &str) -> String {
    s.replace('&', "%26").replace('=', "%3D").replace(' ', "+")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_infocard_contains_title_and_body() {
        let html = render_infocard("Hello", "World");
        assert!(html.contains("Hello"));
        assert!(html.contains("World"));
    }
}
