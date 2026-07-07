use std::collections::HashMap;

use hiqlite::Client;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::db::schema::{AgentHarnessConfig, AgentType, ScopeType, UserModelConfig};
use crate::services::agent_configs;
use crate::services::config_format;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentModelSetting {
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
    config_format::validate(config_body).map_err(|e| e.to_string())?;

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

    get_model_config(client, id)
        .await
        .map_err(|e| e.to_string())
}

pub async fn update_model_config(
    client: &Client,
    user_id: Uuid,
    id: Uuid,
    name: &str,
    config_body: &str,
    harness: &str,
) -> Result<Option<UserModelConfig>, String> {
    config_format::validate(config_body).map_err(|e| e.to_string())?;

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

    get_model_config(client, id)
        .await
        .map(Some)
        .map_err(|e| e.to_string())
}

pub async fn delete_model_config(client: &Client, user_id: Uuid, id: Uuid) -> Result<bool, String> {
    let rows = client
        .execute(
            "DELETE FROM user_model_configs WHERE id = $1 AND user_id = $2",
            hiqlite::params!(id.to_string(), user_id.to_string()),
        )
        .await
        .map_err(|e| e.to_string())?;
    Ok(rows > 0)
}

async fn get_model_config(client: &Client, id: Uuid) -> Result<UserModelConfig, hiqlite::Error> {
    client
        .query_map_one::<UserModelConfig, _>(
            "SELECT id, user_id, name, config_body, harness, created_at, updated_at \
             FROM user_model_configs WHERE id = $1",
            hiqlite::params!(id.to_string()),
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
        map.insert(
            c.agent_type.to_string(),
            AgentModelSetting {
                model: c.model,
                effort: c.effort,
            },
        );
    }
    Ok(map)
}

pub async fn upsert_agent_models(
    client: &Client,
    user_id: Uuid,
    models: HashMap<String, AgentModelSetting>,
) -> Result<HashMap<String, AgentModelSetting>, String> {
    for (agent_type_str, setting) in &models {
        let agent_type: AgentType = agent_type_str
            .parse()
            .map_err(|e: String| format!("invalid agent type '{agent_type_str}': {e}"))?;

        agent_configs::create_or_update_agent_config(
            client,
            &agent_type,
            "openai",
            "",
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
