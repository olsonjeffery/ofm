use leptos::prelude::*;

use crate::webapp::components::api_key_manager::ApiKeyManager;
use crate::webapp::components::settings_sidebar::{
    SettingsSection, SettingsSidebar, SettingsSubPage,
};
use crate::webapp::pages::onboarding::OnboardingForm;

pub fn render(
    active: SettingsSubPage,
    git_name: String,
    git_email: String,
    is_technical: bool,
) -> String {
    let sidebar = leptos::view! {
        <SettingsSidebar section=SettingsSection::Account active />
    }
    .to_html();

    let (pane, js) = match active {
        SettingsSubPage::UserConfig => (
            leptos::view! { <OnboardingForm git_name git_email is_technical /> }.to_html(),
            "",
        ),
        SettingsSubPage::ApiKeys => (
            leptos::view! { <div class="box"><ApiKeyManager/></div> }.to_html(),
            API_KEYS_JS,
        ),
        _ => unreachable!("account page only renders its own sub-pages"),
    };

    format!(
        r#"<section class="section">
            <h2 class="title is-3">
                <span class="icon is-medium"><i class="mdi mdi-account-cog"></i></span>
                "Account"
            </h2>
            <div class="columns">
                <div class="column is-one-quarter">
                    {}
                </div>
                <div class="column">
                    {}
                </div>
            </div>
        </section>
        <script>{}</script>"#,
        sidebar, pane, js
    )
}

const API_KEYS_JS: &str = r#"
window.__ACCESS_TOKEN__ = '';

document.addEventListener('DOMContentLoaded', function() {
    var generateBtn = document.getElementById('btn-generate-key');
    var revokeBtn = document.getElementById('btn-revoke-key');
    var display = document.getElementById('api-key-display');
    var keyValue = document.getElementById('api-key-value');
    var empty = document.getElementById('api-key-empty');

    if (generateBtn) {
        generateBtn.addEventListener('click', function() {
            apiCall('/api/auth/api-key', { method: 'POST' })
                .then(function(r) { return r.json(); })
                .then(function(data) {
                    if (data.api_key) {
                        keyValue.textContent = data.api_key;
                        display.style.display = 'block';
                        empty.style.display = 'none';
                        generateBtn.style.display = 'none';
                        revokeBtn.style.display = 'inline-block';
                    }
                })
                .catch(function(err) {
                    alert('Error generating key: ' + err.message);
                });
        });
    }

    if (revokeBtn) {
        revokeBtn.addEventListener('click', function() {
            apiCall('/api/auth/api-key', { method: 'DELETE' })
                .then(function(r) {
                    if (!r.ok) throw new Error('Revoke failed');
                    display.style.display = 'none';
                    empty.style.display = 'block';
                    generateBtn.style.display = 'inline-block';
                    revokeBtn.style.display = 'none';
                })
                .catch(function(err) {
                    alert('Error: ' + err.message);
                });
        });
    }

    var copyBtn = document.getElementById('btn-copy-key');
    if (copyBtn) {
        copyBtn.addEventListener('click', function() {
            var text = keyValue.textContent;
            navigator.clipboard.writeText(text).then(function() {
                alert('API key copied to clipboard.');
            }).catch(function() {
                alert('Failed to copy. Please copy manually.');
            });
        });
    }
});
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_account_renders_user_config_landing() {
        let html = render(
            SettingsSubPage::UserConfig,
            "Jane".into(),
            "jane@example.com".into(),
            true,
        );
        assert!(html.contains("menu"));
        assert!(html.contains("User Config"));
        assert!(html.contains("Git Name"));
        assert!(html.contains("jane@example.com"));
        assert!(html.contains("onboarding-form"));
        assert!(html.contains("API Keys"));
        assert!(!html.contains("btn-generate-key"));
    }

    #[test]
    fn test_account_renders_api_keys() {
        let html = render(
            SettingsSubPage::ApiKeys,
            "Jane".into(),
            "jane@example.com".into(),
            true,
        );
        assert!(html.contains("menu"));
        assert!(html.contains("API Keys"));
        assert!(html.contains("btn-generate-key"));
        assert!(html.contains("__ACCESS_TOKEN__"));
        assert!(!html.contains("onboarding-form"));
    }
}
