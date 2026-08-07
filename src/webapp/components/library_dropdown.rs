use leptos::prelude::*;

#[component]
pub fn LibraryDropdown() -> impl IntoView {
    view! {
        <div class="navbar-item">
            <div class="dropdown" id="library-dropdown">
                <div class="dropdown-trigger">
                    <div class="buttons has-addons">
                        <button
                            class="button is-white is-small"
                            id="library-dropdown-label"
                            aria-haspopup="true"
                            aria-controls="library-dropdown-menu"
                        >
                            <span class="icon is-small"><i class="mdi mdi-text-box-outline"></i></span>
                            <span>"Library"</span>
                        </button>
                        <button
                            class="button is-white is-small"
                            id="library-dropdown-trigger"
                            aria-haspopup="true"
                            aria-controls="library-dropdown-menu"
                        >
                            <span class="icon is-small"><i class="mdi mdi-arrow-down-bold"></i></span>
                        </button>
                    </div>
                </div>
                <div class="dropdown-menu" id="library-dropdown-menu" role="menu">
                    <div class="dropdown-content">
                        <a class="dropdown-item" href="/webapp/prompts">
                            <span class="icon is-small"><i class="mdi mdi-text-box-outline"></i></span>
                            <span>"Prompts"</span>
                        </a>
                    </div>
                </div>
            </div>
        </div>
        <script>
            {r#"(function(){
                var dd = document.getElementById('library-dropdown');
                var label = document.getElementById('library-dropdown-label');
                var trigger = document.getElementById('library-dropdown-trigger');
                function toggle(ev) {
                    ev.stopPropagation();
                    dd.classList.toggle('is-active');
                }
                if (label) {
                    label.addEventListener('click', toggle);
                }
                if (trigger) {
                    trigger.addEventListener('click', toggle);
                }
                document.addEventListener('click', function(ev) {
                    if (dd && !dd.contains(ev.target)) {
                        dd.classList.remove('is-active');
                    }
                });
            })();"#}
        </script>
        <style>
            {r#"#library-dropdown-label.button,
            #library-dropdown-trigger.button {
                background-color: var(--bulma-black) !important;
                color: var(--bulma-white) !important;
                border: 1px solid var(--bulma-white) !important;
            }
            #library-dropdown-label.button:hover,
            #library-dropdown-trigger.button:hover {
                background-color: var(--bulma-black) !important;
                color: var(--bulma-white) !important;
            }
            #library-dropdown-menu .dropdown-content {
                background: var(--bulma-white-bis);
                color: var(--bulma-grey-darker);
                border: 1px solid var(--bulma-grey);
                border-radius: 3px;
            }"#}
        </style>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_library_dropdown_contains_prompts_link() {
        let html = leptos::view! { <LibraryDropdown /> }.to_html();
        assert!(html.contains("/webapp/prompts"));
        assert!(html.contains("Prompts"));
        assert!(html.contains("mdi-text-box-outline"));
        assert!(html.contains("library-dropdown"));
        assert!(html.contains("library-dropdown-trigger"));
        assert!(html.contains("Library"));
    }
}
