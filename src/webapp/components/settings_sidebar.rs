use leptos::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SettingsSection {
    ProvidersAgents,
    ImportExport,
    Account,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SettingsSubPage {
    ModelConfig,
    AgentSettings,
    Export,
    Import,
    UserConfig,
    ApiKeys,
}

impl SettingsSection {
    fn label(self) -> &'static str {
        match self {
            SettingsSection::ProvidersAgents => "Providers & Agents",
            SettingsSection::ImportExport => "Import/Export",
            SettingsSection::Account => "Account",
        }
    }

    fn items(self) -> &'static [(SettingsSubPage, &'static str)] {
        match self {
            SettingsSection::ProvidersAgents => &[
                (SettingsSubPage::ModelConfig, "Model Configurations"),
                (SettingsSubPage::AgentSettings, "Agent Settings"),
            ],
            SettingsSection::ImportExport => &[
                (SettingsSubPage::Export, "Export"),
                (SettingsSubPage::Import, "Import"),
            ],
            SettingsSection::Account => &[
                (SettingsSubPage::UserConfig, "User Config"),
                (SettingsSubPage::ApiKeys, "API Keys"),
            ],
        }
    }
}

impl SettingsSubPage {
    pub fn href(self) -> &'static str {
        match self {
            SettingsSubPage::ModelConfig => "/webapp/settings/providers-agents/model-config",
            SettingsSubPage::AgentSettings => "/webapp/settings/providers-agents/agent-settings",
            SettingsSubPage::Export => "/webapp/settings/import-export/export",
            SettingsSubPage::Import => "/webapp/settings/import-export/import",
            SettingsSubPage::UserConfig => "/webapp/settings/account/user-config",
            SettingsSubPage::ApiKeys => "/webapp/settings/account/api-keys",
        }
    }
}

#[component]
pub fn SettingsSidebar(section: SettingsSection, active: SettingsSubPage) -> impl IntoView {
    let items = section.items();
    view! {
        <aside class="menu">
            <p class="menu-label">{section.label()}</p>
            <ul class="menu-list">
                {items
                    .iter()
                    .map(move |(subpage, label)| {
                        let href = subpage.href();
                        let is_active = *subpage == active;
                        view! {
                            <li>
                                <a
                                    href=href
                                    class=if is_active { "is-active" } else { "" }
                                >{*label}</a>
                            </li>
                        }
                    })
                    .collect::<Vec<_>>()}
            </ul>
        </aside>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(section: SettingsSection, active: SettingsSubPage) -> String {
        leptos::view! { <SettingsSidebar section active /> }.to_html()
    }

    #[test]
    fn test_providers_agents_section_is_section_local() {
        for active in [SettingsSubPage::ModelConfig, SettingsSubPage::AgentSettings] {
            let html = render(SettingsSection::ProvidersAgents, active);
            assert!(html.contains("menu-label"));
            assert!(html.contains("Model Configurations"));
            assert!(html.contains("Agent Settings"));
            assert!(!html.contains("Export"));
            assert!(!html.contains("Import"));
            assert!(!html.contains("API Keys"));
            assert!(!html.contains("User Config"));
        }
    }

    #[test]
    fn test_import_export_section_is_section_local() {
        for active in [SettingsSubPage::Export, SettingsSubPage::Import] {
            let html = render(SettingsSection::ImportExport, active);
            assert!(html.contains("menu-label"));
            assert!(html.contains("Export"));
            assert!(html.contains("Import"));
            assert!(!html.contains("Model Configurations"));
            assert!(!html.contains("Agent Settings"));
            assert!(!html.contains("API Keys"));
        }
    }

    #[test]
    fn test_account_section_is_section_local() {
        for active in [SettingsSubPage::UserConfig, SettingsSubPage::ApiKeys] {
            let html = render(SettingsSection::Account, active);
            assert!(html.contains("menu-label"));
            assert!(html.contains("User Config"));
            assert!(html.contains("API Keys"));
            assert!(!html.contains("Model Configurations"));
            assert!(!html.contains("Agent Settings"));
            assert!(!html.contains("Export"));
            assert!(!html.contains("Import"));
        }
    }

    #[test]
    fn test_menu_label_matches_section() {
        let html = render(
            SettingsSection::ProvidersAgents,
            SettingsSubPage::ModelConfig,
        );
        assert!(html.contains("Providers"));
        assert!(html.contains("Agents"));

        let html = render(SettingsSection::ImportExport, SettingsSubPage::Export);
        assert!(html.contains("Import/Export"));

        let html = render(SettingsSection::Account, SettingsSubPage::UserConfig);
        assert!(html.contains("Account"));
    }

    #[test]
    fn test_active_link_highlighted_with_correct_href() {
        let cases = [
            (
                SettingsSection::ProvidersAgents,
                SettingsSubPage::ModelConfig,
                "/webapp/settings/providers-agents/model-config",
            ),
            (
                SettingsSection::ProvidersAgents,
                SettingsSubPage::AgentSettings,
                "/webapp/settings/providers-agents/agent-settings",
            ),
            (
                SettingsSection::ImportExport,
                SettingsSubPage::Export,
                "/webapp/settings/import-export/export",
            ),
            (
                SettingsSection::ImportExport,
                SettingsSubPage::Import,
                "/webapp/settings/import-export/import",
            ),
            (
                SettingsSection::Account,
                SettingsSubPage::UserConfig,
                "/webapp/settings/account/user-config",
            ),
            (
                SettingsSection::Account,
                SettingsSubPage::ApiKeys,
                "/webapp/settings/account/api-keys",
            ),
        ];
        for (section, active, expected_href) in cases {
            let html = render(section, active);
            let expected = format!("href=\"{}\" class=\"is-active\"", expected_href);
            assert!(
                html.contains(&expected),
                "expected active link {expected:?} in {html}"
            );
        }
    }

    #[test]
    fn test_exactly_one_active_link() {
        for (section, active) in [
            (
                SettingsSection::ProvidersAgents,
                SettingsSubPage::ModelConfig,
            ),
            (
                SettingsSection::ProvidersAgents,
                SettingsSubPage::AgentSettings,
            ),
            (SettingsSection::ImportExport, SettingsSubPage::Export),
            (SettingsSection::ImportExport, SettingsSubPage::Import),
            (SettingsSection::Account, SettingsSubPage::UserConfig),
            (SettingsSection::Account, SettingsSubPage::ApiKeys),
        ] {
            let html = render(section, active);
            assert_eq!(
                html.matches("class=\"is-active\"").count(),
                1,
                "exactly one is-active link expected for {active:?}: {html}"
            );
        }
    }

    #[test]
    fn test_inactive_links_have_no_is_active() {
        let html = render(
            SettingsSection::ProvidersAgents,
            SettingsSubPage::ModelConfig,
        );
        assert!(html.contains(r#"<a href="/webapp/settings/providers-agents/agent-settings" class="">Agent Settings</a>"#));
        assert!(!html.contains("agent-settings\" class=\"is-active\""));
    }
}
