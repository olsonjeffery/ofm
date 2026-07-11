use hiqlite::Client;
use uuid::Uuid;

use crate::db::schema::{AgentHarnessConfig, AgentType, ScopeType};

#[allow(clippy::too_many_arguments)]
pub async fn create_or_update_agent_config(
    client: &Client,
    agent_type: &AgentType,
    harness: &str,
    provider_config_ref: &str,
    scope_type: &ScopeType,
    user_id: Option<&Uuid>,
    project_id: Option<i64>,
    model: Option<&str>,
    effort: Option<&str>,
) -> Result<AgentHarnessConfig, hiqlite::Error> {
    let now = chrono::Utc::now().naive_utc().to_string();
    let user_id_str = user_id.map(|u| u.to_string());

    let existing = client
        .query_map_one::<AgentHarnessConfig, _>(
            "SELECT id, agent_type, harness, provider_config_ref, scope_type, user_id, project_id, model, effort, created_at, updated_at \
             FROM agent_harness_configs \
             WHERE agent_type = $1 AND scope_type = $2 AND COALESCE(user_id, '') = COALESCE($3, '') AND COALESCE(project_id, -1) = COALESCE($4, -1)",
            hiqlite::params!(agent_type.to_string(), scope_type.to_string(), &user_id_str, project_id),
        )
        .await;

    match existing {
        Ok(config) => {
            client
                .execute(
                    "UPDATE agent_harness_configs SET harness = $1, provider_config_ref = $2, model = $3, effort = $4, updated_at = $5 WHERE id = $6",
                    hiqlite::params!(harness, provider_config_ref, model, effort, &now, config.id.to_string()),
                )
                .await?;
            get_agent_config(client, &config.id).await
        }
        Err(_) => {
            let id = Uuid::new_v4();
            client
                .execute(
                    "INSERT INTO agent_harness_configs (id, agent_type, harness, provider_config_ref, scope_type, user_id, project_id, model, effort, created_at, updated_at) \
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
                    hiqlite::params!(
                        id.to_string(),
                        agent_type.to_string(),
                        harness,
                        provider_config_ref,
                        scope_type.to_string(),
                        user_id_str,
                        project_id,
                        model,
                        effort,
                        &now,
                        &now
                    ),
                )
                .await?;
            get_agent_config(client, &id).await
        }
    }
}

pub async fn get_agent_config(
    client: &Client,
    id: &Uuid,
) -> Result<AgentHarnessConfig, hiqlite::Error> {
    client
        .query_map_one::<AgentHarnessConfig, _>(
            "SELECT id, agent_type, harness, provider_config_ref, scope_type, user_id, project_id, model, effort, created_at, updated_at FROM agent_harness_configs WHERE id = $1",
            hiqlite::params!(id.to_string()),
        )
        .await
}

pub async fn list_agent_configs(
    client: &Client,
    project_id: Option<i64>,
) -> Result<Vec<AgentHarnessConfig>, hiqlite::Error> {
    client
        .query_map::<AgentHarnessConfig, _>(
            "SELECT id, agent_type, harness, provider_config_ref, scope_type, user_id, project_id, model, effort, created_at, updated_at FROM agent_harness_configs WHERE project_id = $1 OR project_id IS NULL ORDER BY scope_type, agent_type",
            hiqlite::params!(project_id),
        )
        .await
}

pub async fn delete_agent_config(client: &Client, id: &Uuid) -> Result<bool, hiqlite::Error> {
    let rows = client
        .execute(
            "DELETE FROM agent_harness_configs WHERE id = $1",
            hiqlite::params!(id.to_string()),
        )
        .await?;
    Ok(rows > 0)
}

pub async fn update_agent_config_model(
    client: &Client,
    id: &Uuid,
    model: &str,
) -> Result<AgentHarnessConfig, hiqlite::Error> {
    let now = chrono::Utc::now().naive_utc().to_string();
    client
        .execute(
            "UPDATE agent_harness_configs SET model = $1, updated_at = $2 WHERE id = $3",
            hiqlite::params!(model, &now, id.to_string()),
        )
        .await?;
    get_agent_config(client, id).await
}
