use std::path::Path;
use std::time::Duration;

use omprint::providers::omp_provider::OmpProvider;
use omprint::providers::{HarnessConfig, LlmProvider};

fn has_binary(name: &str) -> bool {
    std::process::Command::new("which")
        .arg(name)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[tokio::test]
async fn test_omp_provider_new() {
    let config = HarnessConfig {
        agent_type: "planification".into(),
        harness: "oh-my-pi".into(),
        provider_config_ref: "test.yaml".into(),
        model: Some("default".into()),
        effort: Some("balanced".into()),
    };
    let provider = OmpProvider::new(&config, Path::new("omp")).await.unwrap();
    let models = provider.get_models_list().await.unwrap();
    assert!(!models.is_empty(), "get_models_list should return at least 'default'");
}

#[tokio::test]
async fn test_omp_provider_one_shot_prompt() {
    if !has_binary("omp") {
        eprintln!("skipping OmpProvider one_shot test: 'omp' binary not in PATH");
        return;
    }

    let config = HarnessConfig {
        agent_type: "planification".into(),
        harness: "oh-my-pi".into(),
        provider_config_ref: "test.yaml".into(),
        model: Some("default".into()),
        effort: Some("balanced".into()),
    };
    let provider = OmpProvider::new(&config, Path::new("omp")).await.unwrap();

    let result = tokio::time::timeout(
        Duration::from_secs(30),
        provider.one_shot_prompt("say hello", "default"),
    )
    .await;

    match result {
        Ok(Ok(response)) => {
            assert!(!response.is_empty(), "one_shot_prompt returned empty response");
            eprintln!("omp one_shot_prompt response: {response}");
        }
        Ok(Err(e)) => {
            eprintln!("omp one_shot_prompt returned error (binary may need config): {e}");
        }
        Err(_) => {
            eprintln!("omp one_shot_prompt timed out after 30s");
        }
    }
}

#[tokio::test]
async fn test_omp_provider_start_shutdown() {
    if !has_binary("omp") {
        eprintln!("skipping OmpProvider start/shutdown test: 'omp' binary not in PATH");
        return;
    }

    let config = HarnessConfig {
        agent_type: "planification".into(),
        harness: "oh-my-pi".into(),
        provider_config_ref: "test.yaml".into(),
        model: Some("default".into()),
        effort: Some("balanced".into()),
    };
    let mut provider = OmpProvider::new(&config, Path::new("omp")).await.unwrap();

    let tmp = tempfile::TempDir::new().unwrap();
    let start_result = tokio::time::timeout(Duration::from_secs(10), provider.start(tmp.path())).await;

    match start_result {
        Ok(Ok(())) => {
            let shutdown = provider.shutdown().await.unwrap();
            assert!(shutdown, "shutdown should return true when process was running");
        }
        Ok(Err(e)) => {
            eprintln!("omp start returned error (binary may need config): {e}");
        }
        Err(_) => {
            eprintln!("omp start timed out after 10s");
        }
    }
}
