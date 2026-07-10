use std::path::Path;
use std::str::FromStr;

use hiqlite::Client;
use uuid::Uuid;

use crate::db::schema::{AgentHarnessConfig, AgentType, ScopeType};
use crate::providers::config::ProviderConfigDir;
use crate::providers::oh_my_pi::provider::OhMyPiProvider;
use crate::providers::opencode_provider::OpenCodeProvider;
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
    omp_binary: &Path,
    config_root: &Path,
) -> Result<Box<dyn LlmProvider>, ProviderError> {
    match config.harness.as_str() {
        "oh-my-pi" => OhMyPiProvider::new(config, omp_binary, config_root)
            .await
            .map(|p| Box::new(p) as Box<dyn LlmProvider>),
        "opencode" => OpenCodeProvider::new(config, config_root)
            .await
            .map(|p| Box::new(p) as Box<dyn LlmProvider>),
        other => Err(ProviderError::Protocol(format!("unknown harness: {other}"))),
    }
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
        match result {
            Ok(cfg) => results.push(AgentConfigStatus {
                agent_type: at_str.to_string(),
                configured: true,
                scope: Some(cfg.scope.to_string()),
                label: cfg.model,
            }),
            Err(_) => results.push(AgentConfigStatus {
                agent_type: at_str.to_string(),
                configured: false,
                scope: None,
                label: None,
            }),
        }
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
    match result {
        Ok(config) => Ok(Some(config)),
        Err(_) => Ok(None),
    }
}

pub async fn get_models_for_config(
    config_root: &Path,
    config_ref: &str,
) -> Result<Vec<String>, ProviderError> {
    let cfg_dir = ProviderConfigDir::new(config_root);
    let provider_cfg = cfg_dir.load_provider_config(config_ref)?;
    let harness = provider_cfg.harness.as_str();
    let config = HarnessConfig {
        agent_type: "planification".to_string(),
        harness: harness.to_string(),
        provider_config_ref: config_ref.to_string(),
        model: None,
        effort: None,
        scope: ScopeType::Project,
    };
    let provider = resolve_provider(&config, Path::new("omp"), config_root).await?;
    provider.get_models_list().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

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
                    "oh-my-pi",
                    "global.yaml",
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
        assert_eq!(result.harness, "oh-my-pi");

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
                    "oh-my-pi",
                    "user-config.yaml",
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
        let result =
            resolve_harness_config(&client, &agent_type, Some(&user_id), None)
                .await
                .unwrap();
        assert_eq!(result.harness, "oh-my-pi");
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
                    "oh-my-pi",
                    "test.yaml",
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
