use std::collections::HashMap;
use std::path::Path;

use hiqlite::Client;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::db::schema::{AgentHarnessConfig, AgentType, ScopeType, UserModelConfig};
use crate::providers;
use crate::providers::config::ProviderConfigDir;
use crate::services::agent_configs;
use crate::services::config_format;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentModelSetting {
    pub model_config_id: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
}

pub async fn list_model_configs(
    client: &Client,
    user_id: Uuid,
) -> Result<Vec<UserModelConfig>, hiqlite::Error> {
    client
        .query_map::<UserModelConfig, _>(
            "SELECT id, user_id, name, config_body, harness, created_at, updated_at \
             FROM user_model_configs WHERE user_id = $1 ORDER BY created_at",
            hiqlite::params!(user_id.to_string()),
        )
        .await
}

pub async fn create_model_config(
    client: &Client,
    user_id: Uuid,
    name: &str,
    config_body: &str,
    harness: &str,
) -> Result<UserModelConfig, String> {
    config_format::validate_for_harness(config_body, harness).map_err(|e| e.to_string())?;

    let id = Uuid::new_v4();
    let now = chrono::Utc::now().naive_utc().to_string();
    client
        .execute(
            "INSERT INTO user_model_configs (id, user_id, name, config_body, harness, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
            hiqlite::params!(
                id.to_string(),
                user_id.to_string(),
                name,
                config_body,
                harness,
                &now,
                &now
            ),
        )
        .await
        .map_err(|e| e.to_string())?;

    get_model_config(client, user_id, id)
        .await
        .map_err(|e| e.to_string())
}

pub async fn update_model_config(
    client: &Client,
    user_id: Uuid,
    config_root: &Path,
    id: Uuid,
    name: &str,
    config_body: &str,
    harness: &str,
) -> Result<Option<UserModelConfig>, String> {
    config_format::validate_for_harness(config_body, harness).map_err(|e| e.to_string())?;

    let now = chrono::Utc::now().naive_utc().to_string();
    let rows = client
        .execute(
            "UPDATE user_model_configs SET name = $1, config_body = $2, harness = $3, updated_at = $4 \
             WHERE id = $5 AND user_id = $6",
            hiqlite::params!(name, config_body, harness, &now, id.to_string(), user_id.to_string()),
        )
        .await
        .map_err(|e| e.to_string())?;

    if rows == 0 {
        return Ok(None);
    }

    sync_provider_config_file(config_root, &id, config_body);

    get_model_config(client, user_id, id)
        .await
        .map(Some)
        .map_err(|e| e.to_string())
}

fn with_existing_config<F>(config_root: &Path, id: &Uuid, f: F)
where
    F: Fn(&str) -> Result<(), providers::ProviderError>,
{
    let id_str = id.to_string();
    let cfg_dir = ProviderConfigDir::new(config_root);
    for ext in &["yaml", "yml", "json"] {
        let filename = format!("{}.{}", id_str, ext);
        if cfg_dir.config_path(&filename).exists() {
            if let Err(e) = f(&filename) {
                tracing::warn!("Failed to update config file '{}': {e:?}", filename);
            }
            break;
        }
    }
}

fn sync_provider_config_file(config_root: &Path, id: &Uuid, config_body: &str) {
    with_existing_config(config_root, id, |filename| {
        ProviderConfigDir::new(config_root).write_provider_config(filename, config_body)
    });
}

fn remove_provider_config_file(config_root: &Path, id: &Uuid) {
    with_existing_config(config_root, id, |filename| {
        ProviderConfigDir::new(config_root).delete_provider_config(filename)
    });
}

pub async fn delete_model_config(
    client: &Client,
    user_id: Uuid,
    config_root: &Path,
    id: Uuid,
) -> Result<bool, String> {
    let rows = client
        .execute(
            "DELETE FROM user_model_configs WHERE id = $1 AND user_id = $2",
            hiqlite::params!(id.to_string(), user_id.to_string()),
        )
        .await
        .map_err(|e| e.to_string())?;

    if rows > 0 {
        remove_provider_config_file(config_root, &id);
    }

    Ok(rows > 0)
}

async fn get_model_config(
    client: &Client,
    user_id: Uuid,
    id: Uuid,
) -> Result<UserModelConfig, hiqlite::Error> {
    client
        .query_map_one::<UserModelConfig, _>(
            "SELECT id, user_id, name, config_body, harness, created_at, updated_at \
             FROM user_model_configs WHERE id = $1 AND user_id = $2",
            hiqlite::params!(id.to_string(), user_id.to_string()),
        )
        .await
}

pub async fn get_agent_models(
    client: &Client,
    user_id: Uuid,
) -> Result<HashMap<String, AgentModelSetting>, hiqlite::Error> {
    let configs = client
        .query_map::<AgentHarnessConfig, _>(
            "SELECT id, agent_type, harness, provider_config_ref, scope_type, user_id, project_id, model, effort, created_at, updated_at \
             FROM agent_harness_configs \
             WHERE scope_type = 'user' AND user_id = $1",
            hiqlite::params!(user_id.to_string()),
        )
        .await?;

    let mut map = HashMap::new();
    for c in configs {
        let model_config_id = parse_model_config_id(&c.provider_config_ref);
        map.insert(
            c.agent_type.to_string(),
            AgentModelSetting {
                model_config_id,
                model: c.model,
                effort: c.effort,
            },
        );
    }
    Ok(map)
}

fn parse_model_config_id(provider_config_ref: &str) -> Option<String> {
    // provider_config_ref is stored as "{uuid}.{yaml|json}"
    // Strip the extension to get the UUID back
    let stripped = provider_config_ref
        .strip_suffix(".yaml")
        .or_else(|| provider_config_ref.strip_suffix(".yml"))
        .or_else(|| provider_config_ref.strip_suffix(".json"))?;
    // Validate it's a UUID
    Uuid::parse_str(stripped).ok()?;
    Some(stripped.to_string())
}

pub async fn upsert_agent_models(
    client: &Client,
    user_id: Uuid,
    config_root: &Path,
    models: HashMap<String, AgentModelSetting>,
) -> Result<HashMap<String, AgentModelSetting>, String> {
    for (agent_type_str, setting) in &models {
        let agent_type: AgentType = agent_type_str
            .parse()
            .map_err(|e: String| format!("invalid agent type '{agent_type_str}': {e}"))?;

        let (harness, provider_config_ref) = if let Some(ref cfg_id) = setting.model_config_id {
            match Uuid::parse_str(cfg_id) {
                Ok(uuid) => match get_model_config(client, user_id, uuid).await {
                    Ok(model_cfg) => {
                        let ext = if model_cfg.harness == "oh-my-pi" {
                            "yaml"
                        } else {
                            "json"
                        };
                        let filename = format!("{}.{}", model_cfg.id, ext);
                        let cfg_dir = ProviderConfigDir::new(config_root);
                        if let Err(e) =
                            cfg_dir.write_provider_config(&filename, &model_cfg.config_body)
                        {
                            tracing::warn!("Failed to write provider config '{}': {e}", filename);
                        }
                        (model_cfg.harness, filename)
                    }
                    Err(_) => {
                        tracing::warn!(
                            "Model config {cfg_id} not found for agent type '{agent_type_str}'"
                        );
                        (String::new(), String::new())
                    }
                },
                Err(_) => {
                    tracing::warn!(
                        "Invalid model_config_id '{cfg_id}' for agent type '{agent_type_str}'"
                    );
                    (String::new(), String::new())
                }
            }
        } else {
            (String::new(), String::new())
        };

        agent_configs::create_or_update_agent_config(
            client,
            &agent_type,
            &harness,
            &provider_config_ref,
            &ScopeType::User,
            Some(&user_id),
            None,
            setting.model.as_deref(),
            setting.effort.as_deref(),
        )
        .await
        .map_err(|e| e.to_string())?;
    }

    get_agent_models(client, user_id)
        .await
        .map_err(|e| e.to_string())
}
