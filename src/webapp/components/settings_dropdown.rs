use leptos::prelude::*;

#[component]
pub fn SettingsDropdown(is_admin: bool) -> impl IntoView {
    view! {
        <div class="navbar-item">
            <div class="dropdown" id="settings-dropdown">
                <div class="dropdown-trigger">
                    <div class="buttons has-addons">
                        <button
                            class="button is-white is-small"
                            id="settings-dropdown-label"
                            aria-haspopup="true"
                            aria-controls="settings-dropdown-menu"
                        >
                            <span class="icon is-small"><i class="mdi mdi-cog"></i></span>
                            <span>"Settings"</span>
                        </button>
                        <button
                            class="button is-white is-small"
                            id="settings-dropdown-trigger"
                            aria-haspopup="true"
                            aria-controls="settings-dropdown-menu"
                        >
                            <span class="icon is-small"><i class="mdi mdi-arrow-down-bold"></i></span>
                        </button>
                    </div>
                </div>
                <div class="dropdown-menu" id="settings-dropdown-menu" role="menu">
                    <div class="dropdown-content">
                        <a class="dropdown-item" href="/webapp/settings/providers-agents">
                            <span class="icon is-small"><i class="mdi mdi-cog-outline"></i></span>
                            <span>"Providers & Agents"</span>
                        </a>
                        <a class="dropdown-item" href="/webapp/settings/import-export">
                            <span class="icon is-small"><i class="mdi mdi-export-variant"></i></span>
                            <span>"Import/Export"</span>
                        </a>
                        <a class="dropdown-item" href="/webapp/settings/account">
                            <span class="icon is-small"><i class="mdi mdi-account-cog"></i></span>
                            <span>"Account"</span>
                        </a>
                        {if is_admin {
                            view! {
                                <a class="dropdown-item" id="settings-system-item" href="/webapp/system">
                                    <span class="icon is-small"><i class="mdi mdi-heart-pulse"></i></span>
                                    <span>"System"</span>
                                </a>
                            }
                                .into_any()
                        } else {
                            ().into_any()
                        }}
                    </div>
                </div>
            </div>
        </div>
        <script>
            {r#"(function(){
                var dd = document.getElementById('settings-dropdown');
                var label = document.getElementById('settings-dropdown-label');
                var trigger = document.getElementById('settings-dropdown-trigger');
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
            {r#"#settings-dropdown-label.button,
            #settings-dropdown-trigger.button {
                background-color: var(--bulma-black) !important;
                color: var(--bulma-white) !important;
                border: 1px solid var(--bulma-white) !important;
            }
            #settings-dropdown-label.button:hover,
            #settings-dropdown-trigger.button:hover {
                background-color: var(--bulma-black) !important;
                color: var(--bulma-white) !important;
            }
            #settings-dropdown-menu .dropdown-content {
                background: var(--bulma-white-bis);
                color: var(--bulma-grey-darker);
                border: 1px solid var(--bulma-grey);
                border-radius: 3px;
            }"#}
        </style>
    }
}
