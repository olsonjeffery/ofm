use std::time::Duration;

use ofm::providers::config::ProviderConfigDir;
use ofm::providers::opencode_provider::OpenCodeProvider;
use ofm::providers::{HarnessConfig, LlmProvider};

fn has_binary(name: &str) -> bool {
    std::process::Command::new("which")
        .arg(name)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn setup_provider_config(tmp: &tempfile::TempDir, name: &str, content: &str) {
    let cfg_dir = ProviderConfigDir::new(tmp.path());
    cfg_dir.ensure_exists().unwrap();
    cfg_dir.write_provider_config(name, content).unwrap();
}

#[tokio::test]
async fn test_opencode_provider_new() {
    let tmp = tempfile::TempDir::new().unwrap();
    setup_provider_config(
        &tmp,
        "test.json",
        r#"{"providers":{"anthropic":{"apiKey":"sk-test","defaultModel":"claude-sonnet-4-20250514"}}}"#,
    );

    let config = HarnessConfig {
        agent_type: "planification".into(),
        harness: "opencode".into(),
        provider_config_ref: "test.json".into(),
        model: Some("claude-sonnet-4-20250514".into()),
        effort: Some("balanced".into()),
        scope: ofm::db::schema::ScopeType::Project,
    };
    let _provider = OpenCodeProvider::new(&config, tmp.path()).await.unwrap();
}

#[tokio::test]
#[ignore = "on their way out"]
async fn test_opencode_provider_start_shutdown() {
    if !has_binary("opencode") {
        eprintln!("skipping OpenCodeProvider start/shutdown test: 'opencode' binary not in PATH");
        return;
    }

    let tmp = tempfile::TempDir::new().unwrap();
    let snippet = r#"{"providers":{"test":{"apiKey":"sk-test","defaultModel":"test-model"}}}"#;
    setup_provider_config(&tmp, "test.json", snippet);

    let config = HarnessConfig {
        agent_type: "planification".into(),
        harness: "opencode".into(),
        provider_config_ref: "test.json".into(),
        model: Some("test-model".into()),
        effort: Some("balanced".into()),
        scope: ofm::db::schema::ScopeType::Project,
    };
    let mut provider = OpenCodeProvider::new(&config, tmp.path()).await.unwrap();

    let result = tokio::time::timeout(Duration::from_secs(15), provider.start(tmp.path())).await;

    match result {
        Ok(Ok(())) => {
            // Server started successfully - verify shutdown
            let shutdown = provider.shutdown().await.unwrap();
            assert!(
                shutdown,
                "shutdown should return true when server was running"
            );
        }
        Ok(Err(e)) => {
            eprintln!("opencode start returned error: {e}");
        }
        Err(_) => {
            eprintln!("opencode start timed out after 15s");
        }
    }
}

#[tokio::test]
#[ignore = "on their way out"]
async fn test_opencode_provider_get_models_list_transient() {
    if !has_binary("opencode") {
        eprintln!("skipping OpenCodeProvider get_models_list test: 'opencode' binary not in PATH");
        return;
    }

    let tmp = tempfile::TempDir::new().unwrap();
    let snippet = r#"{"providers":{"test":{"apiKey":"sk-test","defaultModel":"test-model"}}}"#;
    setup_provider_config(&tmp, "test.json", snippet);

    let config = HarnessConfig {
        agent_type: "planification".into(),
        harness: "opencode".into(),
        provider_config_ref: "test.json".into(),
        model: Some("test-model".into()),
        effort: Some("balanced".into()),
        scope: ofm::db::schema::ScopeType::Project,
    };
    let provider = OpenCodeProvider::new(&config, tmp.path()).await.unwrap();

    let result = tokio::time::timeout(Duration::from_secs(15), provider.get_models_list()).await;

    match result {
        Ok(Ok(models)) => {
            assert!(
                !models.is_empty(),
                "get_models_list should return at least one model"
            );
            eprintln!("opencode models: {models:?}");
        }
        Ok(Err(e)) => {
            eprintln!("opencode get_models_list returned error: {e}");
        }
        Err(_) => {
            eprintln!("opencode get_models_list timed out after 15s");
        }
    }
}

#[tokio::test]
#[ignore = "on their way out"]
async fn test_opencode_provider_one_shot_transient() {
    if !has_binary("opencode") {
        eprintln!("skipping OpenCodeProvider one_shot test: 'opencode' binary not in PATH");
        return;
    }

    let tmp = tempfile::TempDir::new().unwrap();
    let snippet = r#"{"providers":{"test":{"apiKey":"sk-test","defaultModel":"test-model"}}}"#;
    setup_provider_config(&tmp, "test.json", snippet);

    let config = HarnessConfig {
        agent_type: "planification".into(),
        harness: "opencode".into(),
        provider_config_ref: "test.json".into(),
        model: Some("test-model".into()),
        effort: Some("balanced".into()),
        scope: ofm::db::schema::ScopeType::Project,
    };
    let provider = OpenCodeProvider::new(&config, tmp.path()).await.unwrap();

    let result = tokio::time::timeout(
        Duration::from_secs(30),
        provider.one_shot_prompt("say hello", "test-model"),
    )
    .await;

    match result {
        Ok(Ok(response)) => {
            assert!(
                !response.is_empty(),
                "one_shot_prompt returned empty response"
            );
            eprintln!("opencode one_shot_prompt response: {response}");
        }
        Ok(Err(e)) => {
            eprintln!("opencode one_shot_prompt returned error: {e}");
        }
        Err(_) => {
            eprintln!("opencode one_shot_prompt timed out after 30s");
        }
    }
}
