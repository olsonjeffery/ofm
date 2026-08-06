use std::path::Path;
use std::str::FromStr;

use hiqlite::Client;
use uuid::Uuid;

use crate::db::schema::{AgentHarnessConfig, AgentType, ScopeType};
use crate::providers::config::ProviderConfigDir;
use crate::providers::opencode_sdk_provider::OpenCodeSdkProvider;
use crate::providers::rig_config::{ModelListMode, RigProviderConfig};
use crate::providers::{HarnessConfig, LlmProvider, ProviderError};

#[derive(Debug, Clone, serde::Serialize)]
pub struct AgentConfigStatus {
    pub agent_type: String,
    pub configured: bool,
    pub scope: Option<String>,
    pub label: Option<String>,
}

pub async fn resolve_provider(
    config: &HarnessConfig,
    config_root: &Path,
    log_data: bool,
    footprint: &Path,
) -> Result<Box<dyn LlmProvider>, ProviderError> {
    match config.harness.as_str() {
        "opencode" => OpenCodeSdkProvider::new(config, config_root, log_data, footprint)
            .await
            .map(|p| Box::new(p) as Box<dyn LlmProvider>),
        other => Err(unsupported_harness_error(config, other)),
    }
}

/// Resolve a provider and tag it with the calling user's id. The user_id
/// is used by `OpenCodeSdkProvider` to key the per-user server pool
/// (see `src/opencode_sdk/pool.rs`).
pub async fn resolve_provider_for_user(
    config: &HarnessConfig,
    config_root: &Path,
    user_id: Uuid,
    log_data: bool,
    footprint: &Path,
) -> Result<Box<dyn LlmProvider>, ProviderError> {
    match config.harness.as_str() {
        "opencode" => {
            let p = OpenCodeSdkProvider::new(config, config_root, log_data, footprint).await?;
            p.set_user_id(user_id);
            Ok(Box::new(p) as Box<dyn LlmProvider>)
        }
        other => Err(unsupported_harness_error(config, other)),
    }
}

/// A Rig-selected agent config must not run until RIG 1 lands. Surface a clear
/// "not yet executable" state instead of a confusing unknown-harness error.
fn unsupported_harness_error(config: &HarnessConfig, other: &str) -> ProviderError {
    if other == "rig" {
        ProviderError::Protocol(rig_not_yet_executable_message(&config.provider_config_ref))
    } else {
        ProviderError::Protocol(format!("unknown harness: {other}"))
    }
}

/// The user-facing message for a rig-selected agent config that cannot be
/// executed yet. Shared by the registry guard and the orchestrator's early
/// run guard so the wording stays consistent.
pub fn rig_not_yet_executable_message(provider_config_ref: &str) -> String {
    format!(
        "agent config '{provider_config_ref}' uses the 'rig' harness, which is captured in config but not yet executable (RIG 1 pending)"
    )
}

pub async fn resolve_harness_config(
    db: &Client,
    agent_type: &AgentType,
    user_id: Option<&Uuid>,
    project_id: Option<i64>,
) -> Result<HarnessConfig, ProviderError> {
    let scopes: [(ScopeType, Option<&Uuid>, Option<i64>); 4] = [
        (ScopeType::UserProject, user_id, project_id),
        (ScopeType::Project, None, project_id),
        (ScopeType::User, user_id, None),
        (ScopeType::Global, None, None),
    ];
    for (scope, scope_user, scope_project) in &scopes {
        if let Some(config) =
            lookup_config(db, agent_type, scope.clone(), *scope_user, *scope_project).await?
        {
            if config.model.is_none() {
                return Err(ProviderError::Config(format!(
                    "provider config '{}' for agent type '{agent_type}' scope '{:?}' has no model selected",
                    config.provider_config_ref, config.scope_type
                )));
            }
            return Ok(HarnessConfig {
                agent_type: agent_type.to_string(),
                harness: config.harness,
                provider_config_ref: config.provider_config_ref,
                model: config.model,
                effort: config.effort,
                scope: scope.clone(),
            });
        }
    }
    Err(ProviderError::Protocol(format!(
        "no provider config found for agent type '{agent_type}'"
    )))
}

pub async fn resolve_agent_config_statuses(
    db: &Client,
    user_id: Uuid,
    project_id: i64,
) -> Vec<AgentConfigStatus> {
    let agent_types = [
        "planification",
        "implementation",
        "refinement",
        "review",
        "pr",
    ];
    let mut results = Vec::new();
    for at_str in &agent_types {
        let agent_type = match AgentType::from_str(at_str) {
            Ok(t) => t,
            Err(_) => continue,
        };
        let result =
            resolve_harness_config(db, &agent_type, Some(&user_id), Some(project_id)).await;
        let (configured, scope, label) = match result {
            Ok(cfg) => (true, Some(cfg.scope.to_string()), cfg.model),
            Err(_) => (false, None, None),
        };
        results.push(AgentConfigStatus {
            agent_type: at_str.to_string(),
            configured,
            scope,
            label,
        });
    }
    results
}

async fn lookup_config(
    db: &Client,
    agent_type: &AgentType,
    scope_type: ScopeType,
    user_id: Option<&Uuid>,
    project_id: Option<i64>,
) -> Result<Option<AgentHarnessConfig>, ProviderError> {
    let user_id_str = user_id.map(|u| u.to_string());
    let result = db
        .query_map_one::<AgentHarnessConfig, _>(
            "SELECT id, agent_type, harness, provider_config_ref, scope_type, user_id, project_id, model, effort, created_at, updated_at \
             FROM agent_harness_configs \
             WHERE agent_type = $1 AND scope_type = $2 AND COALESCE(user_id, '') = COALESCE($3, '') AND COALESCE(project_id, -1) = COALESCE($4, -1)",
            hiqlite::params!(agent_type.to_string(), scope_type.to_string(), user_id_str, project_id),
        )
        .await;
    Ok(result.ok())
}

pub async fn get_models_for_config(
    config_root: &Path,
    config_ref: &str,
    log_data: bool,
) -> Result<Vec<String>, ProviderError> {
    let cfg_dir = ProviderConfigDir::new(config_root);
    let provider_cfg = cfg_dir.load_provider_config(config_ref)?;
    let harness = provider_cfg.harness.as_str();
    if harness == "rig" {
        // Rig configs keep their model list on the typed config itself. In
        // OpenApiList mode the list is live-fetched from the provider's
        // model-listing API (no provider execution required); on failure — or
        // for manual mode — the saved/cached list on the config is returned.
        // No DB access here, so no cache persistence on this path (the Agent
        // Settings dropdown path persists).
        let rig: RigProviderConfig = serde_json::from_str(&provider_cfg.raw_snippet)
            .map_err(|e| ProviderError::Config(format!("invalid rig config: {e}")))?;
        return match &rig.model_list_mode {
            ModelListMode::Manual(_) => Ok(rig.available_models()),
            ModelListMode::OpenApiList => {
                match crate::providers::rig_models::list_models(&rig).await {
                    Ok(models) if !models.is_empty() => Ok(models),
                    _ => Ok(rig.available_models()),
                }
            }
        };
    }
    let config = HarnessConfig {
        agent_type: "planification".to_string(),
        harness: harness.to_string(),
        provider_config_ref: config_ref.to_string(),
        model: None,
        effort: None,
        scope: ScopeType::Project,
    };
    let provider = resolve_provider(&config, config_root, log_data, Path::new("")).await?;
    provider.get_models_list().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    #[test]
    fn test_resolve_provider_rig_not_executable() {
        let config = HarnessConfig {
            agent_type: "planification".to_string(),
            harness: "rig".to_string(),
            provider_config_ref: "abc.rig.json".to_string(),
            model: Some("gpt-4".into()),
            effort: None,
            scope: ScopeType::User,
        };
        let tmp = tempfile::TempDir::new().unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let result = runtime
            .block_on(async { resolve_provider(&config, tmp.path(), false, Path::new("")).await });
        let err = match result {
            Ok(_) => panic!("expected rig guard error"),
            Err(e) => e.to_string(),
        };
        assert!(
            err.contains("not yet executable"),
            "expected clear guard message, got: {err}"
        );
    }

    #[test]
    fn test_get_models_for_config_rig_returns_saved_models() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cfg_dir = ProviderConfigDir::new(tmp.path());
        cfg_dir.ensure_exists().unwrap();
        let rig = crate::providers::rig_config::RigProviderConfig {
            name: "rig-provider".into(),
            vendor: crate::providers::rig_config::RigVendor::OpenAi,
            base_url: None,
            api_key: Some("sk-123".into()),
            model_list_mode: crate::providers::rig_config::ModelListMode::Manual(vec![
                "gpt-4".into(),
                "gpt-4o".into(),
            ]),
            models: vec!["gpt-4".into()],
        };
        cfg_dir
            .write_provider_config("abc.rig.json", &serde_json::to_string(&rig).unwrap())
            .unwrap();

        let runtime = tokio::runtime::Runtime::new().unwrap();
        let models = runtime
            .block_on(async { get_models_for_config(tmp.path(), "abc.rig.json", false).await });
        let models = models.unwrap();
        assert_eq!(models, vec!["gpt-4", "gpt-4o"]);
    }

    #[tokio::test]
    async fn test_get_models_for_config_rig_live_fetch() {
        use axum::{routing::get, Json as AxumJson, Router};
        use serde_json::json;

        let tmp = tempfile::TempDir::new().unwrap();
        let cfg_dir = ProviderConfigDir::new(tmp.path());
        cfg_dir.ensure_exists().unwrap();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().route(
                    "/models",
                    get(|| async {
                        AxumJson(json!({ "data": [{ "id": "m-a" }, { "id": "m-b" }] }))
                    }),
                ),
            )
            .await
            .unwrap();
        });

        let rig = crate::providers::rig_config::RigProviderConfig {
            name: "live-provider".into(),
            vendor: crate::providers::rig_config::RigVendor::OpenAiCompatible,
            base_url: Some(format!("http://{addr}")),
            api_key: Some("sk-test".into()),
            model_list_mode: crate::providers::rig_config::ModelListMode::OpenApiList,
            models: vec![],
        };
        cfg_dir
            .write_provider_config("abc.rig.json", &serde_json::to_string(&rig).unwrap())
            .unwrap();

        let models = crate::providers::rig_models::test_env::allow_loopback(async {
            get_models_for_config(tmp.path(), "abc.rig.json", false).await
        })
        .await;
        assert_eq!(models.unwrap(), vec!["m-a", "m-b"]);
    }

    #[tokio::test]
    async fn test_get_models_for_config_rig_live_fetch_falls_back_to_cached() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cfg_dir = ProviderConfigDir::new(tmp.path());
        cfg_dir.ensure_exists().unwrap();

        let rig = crate::providers::rig_config::RigProviderConfig {
            name: "dead-provider".into(),
            vendor: crate::providers::rig_config::RigVendor::OpenAiCompatible,
            base_url: Some("http://127.0.0.1:1".into()),
            api_key: Some("sk-test".into()),
            model_list_mode: crate::providers::rig_config::ModelListMode::OpenApiList,
            models: vec!["cached-1".into()],
        };
        cfg_dir
            .write_provider_config("abc.rig.json", &serde_json::to_string(&rig).unwrap())
            .unwrap();

        let models = crate::providers::rig_models::test_env::allow_loopback(async {
            get_models_for_config(tmp.path(), "abc.rig.json", false).await
        })
        .await;
        assert_eq!(models.unwrap(), vec!["cached-1"]);
    }

    #[tokio::test]
    async fn test_resolve_no_config_found() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cfg = hiqlite::NodeConfig {
            node_id: 1,
            nodes: vec![hiqlite::Node {
                id: 1,
                addr_raft: "127.0.0.1:0".into(),
                addr_api: "127.0.0.1:0".into(),
            }],
            data_dir: tmp.path().to_str().unwrap().to_string().into(),
            secret_raft: "test-raft-secret-12345".into(),
            secret_api: "test-api-secret-12345".into(),
            ..Default::default()
        };
        let client = hiqlite::start_node(cfg).await.unwrap();
        client.wait_until_healthy_db().await;
        db::run_migrations(&client).await.unwrap();

        let result = resolve_harness_config(&client, &AgentType::Implementation, None, None).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("no provider config found"));
    }

    #[tokio::test]
    async fn test_scope_precedence() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cfg = hiqlite::NodeConfig {
            node_id: 1,
            nodes: vec![hiqlite::Node {
                id: 1,
                addr_raft: "127.0.0.1:0".into(),
                addr_api: "127.0.0.1:0".into(),
            }],
            data_dir: tmp.path().to_str().unwrap().to_string().into(),
            secret_raft: "test-raft-secret-12345".into(),
            secret_api: "test-api-secret-12345".into(),
            ..Default::default()
        };
        let client = hiqlite::start_node(cfg).await.unwrap();
        client.wait_until_healthy_db().await;
        db::run_migrations(&client).await.unwrap();

        let agent_type = AgentType::Implementation;
        let now = chrono::Utc::now().naive_utc().to_string();

        let id1 = Uuid::new_v4().to_string();
        client
            .execute(
                "INSERT INTO agent_harness_configs (id, agent_type, harness, provider_config_ref, scope_type) VALUES ($1, $2, $3, $4, $5)",
                hiqlite::params!(
                    &id1,
                    agent_type.to_string(),
                    "opencode",
                    "global.json",
                    ScopeType::Global.to_string()
                ),
            )
            .await
            .unwrap();
        client
            .execute(
                "UPDATE agent_harness_configs SET model = $1, effort = $2, created_at = $3, updated_at = $4 WHERE id = $5",
                hiqlite::params!("gpt-4", "balanced", &now, &now, &id1),
            )
            .await
            .unwrap();

        let result = resolve_harness_config(&client, &agent_type, None, None)
            .await
            .unwrap();
        assert_eq!(result.harness, "opencode");

        let user_id = Uuid::new_v4();
        let id2 = Uuid::new_v4().to_string();
        client
            .execute(
                "INSERT INTO agent_harness_configs (id, agent_type, harness, provider_config_ref, scope_type, user_id) VALUES ($1, $2, $3, $4, $5, $6)",
                hiqlite::params!(
                    &id2,
                    agent_type.to_string(),
                    "opencode",
                    "user.json",
                    ScopeType::User.to_string(),
                    user_id.to_string()
                ),
            )
            .await
            .unwrap();
        client
            .execute(
                "UPDATE agent_harness_configs SET model = $1, effort = $2, created_at = $3, updated_at = $4 WHERE id = $5",
                hiqlite::params!("claude-3", "high", &now, &now, &id2),
            )
            .await
            .unwrap();

        let result = resolve_harness_config(&client, &agent_type, Some(&user_id), None)
            .await
            .unwrap();
        assert_eq!(result.harness, "opencode");
        assert_eq!(result.model.as_deref(), Some("claude-3"));
    }

    #[tokio::test]
    async fn test_scope_resolution_with_both_ids() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cfg = hiqlite::NodeConfig {
            node_id: 1,
            nodes: vec![hiqlite::Node {
                id: 1,
                addr_raft: "127.0.0.1:0".into(),
                addr_api: "127.0.0.1:0".into(),
            }],
            data_dir: tmp.path().to_str().unwrap().to_string().into(),
            secret_raft: "test-raft-secret-12345".into(),
            secret_api: "test-api-secret-12345".into(),
            ..Default::default()
        };
        let client = hiqlite::start_node(cfg).await.unwrap();
        client.wait_until_healthy_db().await;
        db::run_migrations(&client).await.unwrap();

        let agent_type = AgentType::Review;
        let now = chrono::Utc::now().naive_utc().to_string();
        let user_id = Uuid::new_v4();

        // Insert a User-scoped config (user_id set, project_id NULL)
        client
            .execute(
                "INSERT INTO agent_harness_configs (id, agent_type, harness, provider_config_ref, scope_type, user_id, model, effort, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
                hiqlite::params!(
                    Uuid::new_v4().to_string(),
                    agent_type.to_string(),
                    "opencode",
                    "user-config.json",
                    ScopeType::User.to_string(),
                    user_id.to_string(),
                    "gpt-4",
                    "medium",
                    &now,
                    &now
                ),
            )
            .await
            .unwrap();

        // Also insert a Global-scoped config with different model
        client
            .execute(
                "INSERT INTO agent_harness_configs (id, agent_type, harness, provider_config_ref, scope_type, model, effort, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
                hiqlite::params!(
                    Uuid::new_v4().to_string(),
                    agent_type.to_string(),
                    "opencode",
                    "global.json",
                    ScopeType::Global.to_string(),
                    "claude-3",
                    "low",
                    &now,
                    &now
                ),
            )
            .await
            .unwrap();

        // Call with both IDs — should find User-scoped config (higher precedence than Global)
        let result = resolve_harness_config(&client, &agent_type, Some(&user_id), None)
            .await
            .unwrap();
        assert_eq!(result.harness, "opencode");
        assert_eq!(result.model.as_deref(), Some("gpt-4"));
        assert_eq!(result.scope, ScopeType::User);

        // Call with only project_id — should find Global (no user scope matches, no project scope)
        let result = resolve_harness_config(&client, &agent_type, None, Some(42))
            .await
            .unwrap();
        assert_eq!(result.harness, "opencode");
        assert_eq!(result.scope, ScopeType::Global);
    }

    #[tokio::test]
    async fn test_config_with_null_model_rejected() {
        let tmp = tempfile::TempDir::new().unwrap();
        let cfg = hiqlite::NodeConfig {
            node_id: 1,
            nodes: vec![hiqlite::Node {
                id: 1,
                addr_raft: "127.0.0.1:0".into(),
                addr_api: "127.0.0.1:0".into(),
            }],
            data_dir: tmp.path().to_str().unwrap().to_string().into(),
            secret_raft: "test-raft-secret-12345".into(),
            secret_api: "test-api-secret-12345".into(),
            ..Default::default()
        };
        let client = hiqlite::start_node(cfg).await.unwrap();
        client.wait_until_healthy_db().await;
        db::run_migrations(&client).await.unwrap();

        let agent_type = AgentType::Implementation;
        let now = chrono::Utc::now().naive_utc().to_string();
        client
            .execute(
                "INSERT INTO agent_harness_configs (id, agent_type, harness, provider_config_ref, scope_type, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7)",
                hiqlite::params!(
                    Uuid::new_v4().to_string(),
                    agent_type.to_string(),
                    "opencode",
                    "test.json",
                    ScopeType::Global.to_string(),
                    &now,
                    &now
                ),
            )
            .await
            .unwrap();

        let result = resolve_harness_config(&client, &agent_type, None, None).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("no model selected"));
    }
}
