use std::collections::HashMap;
use std::path::Path;

use hiqlite::Client;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::db::schema::{AgentHarnessConfig, AgentType, ScopeType, UserModelConfig};
use crate::providers::config::ProviderConfigDir;
use crate::providers::rig_config::{ModelListMode, RigProviderConfig};
use crate::providers::rig_models;
use crate::services::agent_configs;
use crate::services::config_format;

const RIG_HARNESS: &str = "rig";
const OPENCODE_HARNESS: &str = "opencode";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentModelSetting {
    pub model_config_id: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
}

/// A persisted Rig provider config (a `user_model_configs` row with
/// `harness = "rig"`) with its typed config parsed out of `config_body`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RigProviderWithConfig {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub config: RigProviderConfig,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
}

pub async fn create_model_config(
    client: &Client,
    user_id: Uuid,
    config_root: &Path,
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

    // Write the provider config file immediately so the config is selectable
    // and its model listing is enumerable (Agent Settings dropdown) even
    // before the user first saves agent settings.
    write_provider_config_file(config_root, &config_filename(&id, harness), config_body);

    get_model_config(client, user_id, id)
        .await
        .map_err(|e| e.to_string())
}

fn config_filename(id: &Uuid, harness: &str) -> String {
    if harness == RIG_HARNESS {
        format!("{}.rig.json", id)
    } else {
        format!("{}.json", id)
    }
}

fn provider_config_filename(model_cfg: &UserModelConfig) -> String {
    config_filename(&model_cfg.id, &model_cfg.harness)
}

fn write_provider_config_file(config_root: &Path, filename: &str, content: &str) {
    let cfg_dir = ProviderConfigDir::new(config_root);
    if let Err(e) = cfg_dir.write_provider_config(filename, content) {
        tracing::warn!("Failed to write config file '{}': {e:?}", filename);
    }
}

fn delete_provider_config_file(config_root: &Path, filename: &str) {
    let cfg_dir = ProviderConfigDir::new(config_root);
    if let Err(e) = cfg_dir.delete_provider_config(filename) {
        tracing::warn!("Failed to delete config file '{}': {e:?}", filename);
    }
}

fn sync_provider_config_file(config_root: &Path, id: &Uuid, harness: &str, config_body: &str) {
    let cfg_dir = ProviderConfigDir::new(config_root);
    let (target, stale) = if harness == RIG_HARNESS {
        (
            config_filename(id, RIG_HARNESS),
            config_filename(id, OPENCODE_HARNESS),
        )
    } else {
        (
            config_filename(id, OPENCODE_HARNESS),
            config_filename(id, RIG_HARNESS),
        )
    };
    delete_provider_config_file(config_root, &stale);
    if cfg_dir.config_path(&target).exists() {
        write_provider_config_file(config_root, &target, config_body);
    }
}

fn remove_provider_config_file(config_root: &Path, id: &Uuid) {
    delete_provider_config_file(config_root, &config_filename(id, RIG_HARNESS));
    delete_provider_config_file(config_root, &config_filename(id, OPENCODE_HARNESS));
}

pub async fn get_model_config_name(client: &Client, user_id: Uuid, id: Uuid) -> Option<String> {
    get_model_config(client, user_id, id)
        .await
        .ok()
        .map(|c| c.name)
}

/// Fetch a model config by id without an ownership filter. Callers must
/// authorize access (see `services::access::has_model_config_access`).
pub async fn get_model_config_by_id(
    client: &Client,
    id: Uuid,
) -> Result<UserModelConfig, hiqlite::Error> {
    client
        .query_map_one::<UserModelConfig, _>(
            "SELECT id, user_id, name, config_body, harness, created_at, updated_at \
             FROM user_model_configs WHERE id = $1",
            hiqlite::params!(id.to_string()),
        )
        .await
}

/// Update a model config by id without an ownership filter. Callers must
/// authorize access first.
pub async fn update_model_config_by_id(
    client: &Client,
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
             WHERE id = $5",
            hiqlite::params!(name, config_body, harness, &now, id.to_string()),
        )
        .await
        .map_err(|e| e.to_string())?;

    if rows == 0 {
        return Ok(None);
    }

    sync_provider_config_file(config_root, &id, harness, config_body);

    get_model_config_by_id(client, id)
        .await
        .map(Some)
        .map_err(|e| e.to_string())
}

/// Delete a model config by id without an ownership filter. Callers must
/// authorize access first.
pub async fn delete_model_config_by_id(
    client: &Client,
    config_root: &Path,
    id: Uuid,
) -> Result<bool, String> {
    let rows = client
        .execute(
            "DELETE FROM user_model_configs WHERE id = $1",
            hiqlite::params!(id.to_string()),
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
    // provider_config_ref is stored as "{uuid}.json" for opencode file-based
    // configs and "{uuid}.rig.json" for rig configs. Strip the extension to
    // get the UUID back.
    let stripped = provider_config_ref
        .strip_suffix(".rig.json")
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
                        // "Model Configs implement Model Listing": for rig
                        // configs the picked model must be one of the models
                        // saved on the provider config.
                        if model_cfg.harness == RIG_HARNESS {
                            validate_rig_model_selection(&model_cfg, setting)?;
                        }
                        let filename = provider_config_filename(&model_cfg);
                        write_provider_config_file(config_root, &filename, &model_cfg.config_body);
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

fn validate_rig_model_selection(
    model_cfg: &UserModelConfig,
    setting: &AgentModelSetting,
) -> Result<(), String> {
    let Ok(rig) = serde_json::from_str::<RigProviderConfig>(&model_cfg.config_body) else {
        return Err(format!(
            "rig provider config '{}' has an invalid stored config body",
            model_cfg.name
        ));
    };
    let available = rig.available_models();
    if let Some(model) = setting.model.as_deref() {
        if !model.trim().is_empty()
            && !available.is_empty()
            && !available.iter().any(|m| m == model)
        {
            return Err(format!(
                "model '{model}' is not in provider '{}' model list (available: {})",
                rig.name,
                available.join(", ")
            ));
        }
    }
    Ok(())
}

// ── Rig provider config CRUD ─────────────────────────────────────────────────

/// Mask a provider api key for display so the real secret is never re-served
/// to the client: `sk-1234567890` → `••••7890`. The masked value doubles as
/// the "unchanged" sentinel the update path maps back to the stored key.
fn mask_api_key(key: &str) -> String {
    let key = key.trim();
    let last4: String = key.chars().rev().take(4).collect();
    if last4.chars().count() < 4 {
        "••••".to_string()
    } else {
        format!("••••{}", last4.chars().rev().collect::<String>())
    }
}

fn rig_provider_from_row(row: UserModelConfig) -> Result<RigProviderWithConfig, String> {
    let mut config: RigProviderConfig = serde_json::from_str(&row.config_body)
        .map_err(|e| format!("stored rig config '{}' is invalid: {e}", row.name))?;
    config.api_key = config.api_key.as_deref().map(mask_api_key);
    Ok(RigProviderWithConfig {
        id: row.id,
        user_id: row.user_id,
        name: row.name,
        config,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

/// List the current user's saved Rig provider configs.
pub async fn list_rig_providers(
    client: &Client,
    user_id: Uuid,
) -> Result<Vec<RigProviderWithConfig>, String> {
    let rows = client
        .query_map::<UserModelConfig, _>(
            "SELECT id, user_id, name, config_body, harness, created_at, updated_at \
             FROM user_model_configs WHERE user_id = $1 AND harness = 'rig' ORDER BY created_at",
            hiqlite::params!(user_id.to_string()),
        )
        .await
        .map_err(|e| e.to_string())?;
    rows.into_iter()
        .map(|row| rig_provider_from_row(row).map_err(|e| e.to_string()))
        .collect()
}

async fn get_rig_provider_row(
    client: &Client,
    user_id: Uuid,
    id: Uuid,
) -> Result<Option<UserModelConfig>, String> {
    let result = client
        .query_map_one::<UserModelConfig, _>(
            "SELECT id, user_id, name, config_body, harness, created_at, updated_at \
             FROM user_model_configs WHERE id = $1 AND user_id = $2 AND harness = 'rig'",
            hiqlite::params!(id.to_string(), user_id.to_string()),
        )
        .await;
    Ok(result.ok())
}

/// Fetch a single Rig provider config for the user.
pub async fn get_rig_provider(
    client: &Client,
    user_id: Uuid,
    id: Uuid,
) -> Result<Option<RigProviderWithConfig>, String> {
    get_rig_provider_row(client, user_id, id)
        .await?
        .map(rig_provider_from_row)
        .transpose()
        .map_err(|e| e.to_string())
}

/// Create a Rig provider config: validate, persist the row with
/// `harness = "rig"`, and write the `{uuid}.rig.json` config file.
pub async fn create_rig_provider(
    client: &Client,
    user_id: Uuid,
    config_root: &Path,
    cfg: RigProviderConfig,
) -> Result<RigProviderWithConfig, String> {
    cfg.validate()?;
    let config_body = serde_json::to_string(&cfg).map_err(|e| e.to_string())?;
    let id = Uuid::new_v4();
    let now = chrono::Utc::now().naive_utc().to_string();
    client
        .execute(
            "INSERT INTO user_model_configs (id, user_id, name, config_body, harness, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
            hiqlite::params!(
                id.to_string(),
                user_id.to_string(),
                cfg.name.clone(),
                &config_body,
                RIG_HARNESS,
                &now,
                &now
            ),
        )
        .await
        .map_err(|e| e.to_string())?;

    write_provider_config_file(
        config_root,
        &config_filename(&id, RIG_HARNESS),
        &config_body,
    );

    get_rig_provider(client, user_id, id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "created rig provider not found".to_string())
}

/// Update a Rig provider config and resync its config file.
pub async fn update_rig_provider(
    client: &Client,
    user_id: Uuid,
    config_root: &Path,
    id: Uuid,
    mut cfg: RigProviderConfig,
) -> Result<Option<RigProviderWithConfig>, String> {
    let existing = get_rig_provider_row(client, user_id, id).await?;
    let Some(existing_row) = existing else {
        return Ok(None);
    };
    // A client that left the key untouched submits the masked preview from the
    // read response; map it back to the stored secret instead of overwriting it.
    let stored_key = serde_json::from_str::<RigProviderConfig>(&existing_row.config_body)
        .ok()
        .and_then(|c| c.api_key);
    if let Some(submitted) = &cfg.api_key {
        if stored_key.as_deref().map(mask_api_key).as_deref() == Some(submitted.as_str()) {
            cfg.api_key = stored_key;
        }
    }

    cfg.validate()?;
    let config_body = serde_json::to_string(&cfg).map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().naive_utc().to_string();
    let rows = client
        .execute(
            "UPDATE user_model_configs SET name = $1, config_body = $2, updated_at = $3 \
             WHERE id = $4 AND user_id = $5 AND harness = 'rig'",
            hiqlite::params!(
                cfg.name.clone(),
                &config_body,
                &now,
                id.to_string(),
                user_id.to_string()
            ),
        )
        .await
        .map_err(|e| e.to_string())?;
    if rows == 0 {
        return Ok(None);
    }

    write_provider_config_file(
        config_root,
        &config_filename(&id, RIG_HARNESS),
        &config_body,
    );

    get_rig_provider(client, user_id, id)
        .await
        .map_err(|e| e.to_string())
}

/// Delete a Rig provider config and its config file.
pub async fn delete_rig_provider(
    client: &Client,
    user_id: Uuid,
    config_root: &Path,
    id: Uuid,
) -> Result<bool, String> {
    let rows = client
        .execute(
            "DELETE FROM user_model_configs WHERE id = $1 AND user_id = $2 AND harness = 'rig'",
            hiqlite::params!(id.to_string(), user_id.to_string()),
        )
        .await
        .map_err(|e| e.to_string())?;
    if rows > 0 {
        delete_provider_config_file(config_root, &config_filename(&id, RIG_HARNESS));
    }
    Ok(rows > 0)
}

/// The model ids available on a Rig provider config — the source for the Agent
/// Settings model dropdown. In `OpenApiList` mode the list is live-fetched
/// from the provider's model-listing API (see `providers::rig_models`) and, on
/// success, cached back onto the row and the `{uuid}.rig.json` file so
/// downstream checks (`available_models`, selection validation) stay coherent
/// with what the dropdown showed. On failure the callers degrade to the cached
/// list; if there is nothing cached to degrade to, the error is surfaced so the
/// UI can show it instead of a silent empty dropdown.
pub async fn get_rig_provider_models(
    client: &Client,
    user_id: Uuid,
    id: Uuid,
    config_root: &Path,
) -> Result<Vec<String>, String> {
    let row = get_rig_provider_row(client, user_id, id)
        .await?
        .ok_or_else(|| "rig provider not found".to_string())?;
    let mut config: RigProviderConfig =
        serde_json::from_str(&row.config_body).map_err(|e| e.to_string())?;

    if let ModelListMode::OpenApiList = &config.model_list_mode {
        match rig_models::list_models(&config).await {
            Ok(models) => {
                config.models = models.clone();
                persist_rig_models_cache(client, &row, config_root, &config).await;
                return Ok(models);
            }
            Err(e) => {
                tracing::warn!(
                    "rig provider '{}' model listing failed; using cached list: {e}",
                    row.name
                );
                let cached = config.available_models();
                if cached.is_empty() {
                    return Err(format!(
                        "could not load models for rig provider '{}': {e}",
                        row.name
                    ));
                }
                return Ok(cached);
            }
        }
    }
    Ok(config.available_models())
}

/// Best-effort persistence of a freshly fetched model list back into the
/// `user_model_configs` row and the provider config file. Failures are logged,
/// never propagated — a cache-write hiccup must not break the dropdown.
async fn persist_rig_models_cache(
    client: &Client,
    row: &UserModelConfig,
    config_root: &Path,
    config: &RigProviderConfig,
) {
    let config_body = match serde_json::to_string(config) {
        Ok(body) => body,
        Err(e) => {
            tracing::warn!(
                "failed to serialize updated rig config '{}' for cache persist: {e}",
                row.name
            );
            return;
        }
    };
    let now = chrono::Utc::now().naive_utc().to_string();
    if let Err(e) = client
        .execute(
            "UPDATE user_model_configs SET config_body = $1, updated_at = $2 WHERE id = $3 AND user_id = $4 AND harness = 'rig'",
            hiqlite::params!(
                &config_body,
                &now,
                row.id.to_string(),
                row.user_id.to_string()
            ),
        )
        .await
    {
        tracing::warn!(
            "failed to persist rig model list cache for '{}': {e}",
            row.name
        );
        return;
    }
    write_provider_config_file(
        config_root,
        &config_filename(&row.id, RIG_HARNESS),
        &config_body,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tempfile::TempDir;

    struct TestCtx {
        client: hiqlite::Client,
        user_id: Uuid,
        config_root: std::path::PathBuf,
        _tmp: TempDir,
    }

    async fn setup() -> TestCtx {
        let tmp = TempDir::new().unwrap();
        let config = hiqlite::NodeConfig {
            node_id: 1,
            nodes: vec![hiqlite::Node {
                id: 1,
                addr_raft: "127.0.0.1:0".into(),
                addr_api: "127.0.0.1:0".into(),
            }],
            data_dir: tmp.path().to_str().unwrap().to_string().into(),
            secret_raft: "test-raft-secret-123".into(),
            secret_api: "test-api-secret-123".into(),
            ..Default::default()
        };
        let client = hiqlite::start_node(config).await.unwrap();
        client.wait_until_healthy_db().await;
        crate::db::run_migrations(&client).await.unwrap();
        let user_id = crate::db::ensure_default_user(&client).await.unwrap();
        let config_root = tmp.path().join("config");
        std::fs::create_dir_all(&config_root).unwrap();
        TestCtx {
            client,
            user_id,
            config_root,
            _tmp: tmp,
        }
    }

    fn sample_rig(vendor: crate::providers::rig_config::RigVendor) -> RigProviderConfig {
        RigProviderConfig {
            name: "test-provider".into(),
            vendor,
            base_url: Some("https://example.com/v1".into()),
            api_key: Some("sk-123".into()),
            model_list_mode: crate::providers::rig_config::ModelListMode::Manual(vec![
                "gpt-4".into(),
                "gpt-4o".into(),
            ]),
            models: vec!["gpt-4".into()],
        }
    }

    #[tokio::test]
    async fn test_rig_provider_crud_round_trip() {
        let ctx = setup().await;
        let created = create_rig_provider(
            &ctx.client,
            ctx.user_id,
            &ctx.config_root,
            sample_rig(crate::providers::rig_config::RigVendor::OpenAi),
        )
        .await
        .unwrap();
        assert_eq!(created.config.name, "test-provider");
        assert_eq!(
            created.config.vendor,
            crate::providers::rig_config::RigVendor::OpenAi
        );

        // The config file is written as {uuid}.rig.json
        let cfg_dir = ProviderConfigDir::new(&ctx.config_root);
        let filename = format!("{}.rig.json", created.id);
        assert!(cfg_dir.config_path(&filename).exists());
        let loaded = cfg_dir.load_provider_config(&filename).unwrap();
        assert_eq!(loaded.harness, "rig");

        // Listing sees the provider
        let listed = list_rig_providers(&ctx.client, ctx.user_id).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, created.id);

        // Models endpoint returns the manual list
        let models =
            get_rig_provider_models(&ctx.client, ctx.user_id, created.id, &ctx.config_root)
                .await
                .unwrap();
        assert_eq!(models, vec!["gpt-4", "gpt-4o"]);

        // Update the provider
        let mut updated_cfg = created.config.clone();
        updated_cfg.name = "renamed".into();
        let updated = update_rig_provider(
            &ctx.client,
            ctx.user_id,
            &ctx.config_root,
            created.id,
            updated_cfg,
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(updated.name, "renamed");

        // Delete removes the row and the file
        let deleted = delete_rig_provider(&ctx.client, ctx.user_id, &ctx.config_root, created.id)
            .await
            .unwrap();
        assert!(deleted);
        assert!(!cfg_dir.config_path(&filename).exists());
        assert!(list_rig_providers(&ctx.client, ctx.user_id)
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn test_create_rig_provider_rejects_invalid() {
        let ctx = setup().await;
        let mut cfg = sample_rig(crate::providers::rig_config::RigVendor::OpenAi);
        cfg.model_list_mode = crate::providers::rig_config::ModelListMode::Manual(vec![]);
        let result = create_rig_provider(&ctx.client, ctx.user_id, &ctx.config_root, cfg).await;
        assert!(result.is_err());
        assert!(list_rig_providers(&ctx.client, ctx.user_id)
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn test_get_rig_provider_models_not_found() {
        let ctx = setup().await;
        let result =
            get_rig_provider_models(&ctx.client, ctx.user_id, Uuid::new_v4(), &ctx.config_root)
                .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }

    /// Insert a rig row directly, bypassing `validate` so a loopback base_url
    /// pointing at an in-test mock upstream is allowed.
    async fn insert_rig_row_direct(ctx: &TestCtx, cfg: &RigProviderConfig) -> Uuid {
        let id = Uuid::new_v4();
        let now = chrono::Utc::now().naive_utc().to_string();
        ctx.client
            .execute(
                "INSERT INTO user_model_configs (id, user_id, name, config_body, harness, created_at, updated_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7)",
                hiqlite::params!(
                    id.to_string(),
                    ctx.user_id.to_string(),
                    cfg.name.clone(),
                    serde_json::to_string(cfg).unwrap(),
                    "rig",
                    &now,
                    &now
                ),
            )
            .await
            .unwrap();
        id
    }

    #[tokio::test]
    async fn test_get_rig_provider_models_live_fetch_persists_cache() {
        use axum::{routing::get, Json as AxumJson, Router};
        use serde_json::json;

        let ctx = setup().await;
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

        let cfg = RigProviderConfig {
            name: "live-provider".into(),
            vendor: crate::providers::rig_config::RigVendor::OpenAiCompatible,
            base_url: Some(format!("http://{addr}")),
            api_key: Some("sk-test".into()),
            model_list_mode: ModelListMode::OpenApiList,
            models: vec![],
        };
        let id = insert_rig_row_direct(&ctx, &cfg).await;

        let models = crate::providers::rig_models::test_env::allow_loopback(async {
            get_rig_provider_models(&ctx.client, ctx.user_id, id, &ctx.config_root).await
        })
        .await;
        let models = models.expect("live fetch should succeed");
        assert_eq!(models, vec!["m-a", "m-b"]);

        // The fetched list is persisted back to the row...
        let row = get_rig_provider_row(&ctx.client, ctx.user_id, id)
            .await
            .unwrap()
            .unwrap();
        let stored: RigProviderConfig = serde_json::from_str(&row.config_body).unwrap();
        assert_eq!(stored.models, vec!["m-a", "m-b"]);

        // ...and to the {uuid}.rig.json config file.
        let cfg_dir = ProviderConfigDir::new(&ctx.config_root);
        let file = cfg_dir
            .load_provider_config(&format!("{id}.rig.json"))
            .unwrap();
        let file_cfg: RigProviderConfig = serde_json::from_str(&file.raw_snippet).unwrap();
        assert_eq!(file_cfg.models, vec!["m-a", "m-b"]);
    }

    #[tokio::test]
    async fn test_get_rig_provider_models_live_fetch_failure_degrades_to_cached() {
        let ctx = setup().await;
        let cfg = RigProviderConfig {
            name: "dead-provider".into(),
            vendor: crate::providers::rig_config::RigVendor::OpenAiCompatible,
            base_url: Some("http://127.0.0.1:1".into()),
            api_key: Some("sk-test".into()),
            model_list_mode: ModelListMode::OpenApiList,
            models: vec!["cached-1".into(), "cached-2".into()],
        };
        let id = insert_rig_row_direct(&ctx, &cfg).await;

        let models = crate::providers::rig_models::test_env::allow_loopback(async {
            get_rig_provider_models(&ctx.client, ctx.user_id, id, &ctx.config_root).await
        })
        .await;
        let models = models.expect("should degrade to cached list");
        assert_eq!(models, vec!["cached-1", "cached-2"]);
    }

    #[tokio::test]
    async fn test_get_rig_provider_models_live_fetch_failure_empty_cache_errors() {
        let ctx = setup().await;
        let cfg = RigProviderConfig {
            name: "dead-empty-provider".into(),
            vendor: crate::providers::rig_config::RigVendor::OpenAiCompatible,
            base_url: Some("http://127.0.0.1:1".into()),
            api_key: Some("sk-test".into()),
            model_list_mode: ModelListMode::OpenApiList,
            models: vec![],
        };
        let id = insert_rig_row_direct(&ctx, &cfg).await;

        let result = crate::providers::rig_models::test_env::allow_loopback(async {
            get_rig_provider_models(&ctx.client, ctx.user_id, id, &ctx.config_root).await
        })
        .await;
        let err = result.expect_err("empty-cache listing failure must surface an error");
        assert!(err.contains("could not load models"), "got: {err}");
    }

    #[tokio::test]
    async fn test_get_rig_provider_models_manual_mode_no_http() {
        let ctx = setup().await;
        let created = create_rig_provider(
            &ctx.client,
            ctx.user_id,
            &ctx.config_root,
            sample_rig(crate::providers::rig_config::RigVendor::OpenAi),
        )
        .await
        .unwrap();
        let models =
            get_rig_provider_models(&ctx.client, ctx.user_id, created.id, &ctx.config_root)
                .await
                .unwrap();
        assert_eq!(models, vec!["gpt-4", "gpt-4o"]);
    }

    #[test]
    fn test_parse_model_config_id_rig() {
        let id = Uuid::new_v4();
        assert_eq!(
            parse_model_config_id(&format!("{}.rig.json", id)).as_deref(),
            Some(id.to_string().as_str())
        );
        assert_eq!(
            parse_model_config_id(&format!("{}.json", id)).as_deref(),
            Some(id.to_string().as_str())
        );
        assert_eq!(parse_model_config_id("global.json"), None);
    }

    #[tokio::test]
    async fn test_upsert_agent_models_with_rig_harness() {
        let ctx = setup().await;
        let created = create_rig_provider(
            &ctx.client,
            ctx.user_id,
            &ctx.config_root,
            sample_rig(crate::providers::rig_config::RigVendor::Anthropic),
        )
        .await
        .unwrap();

        let mut models = HashMap::new();
        models.insert(
            "implementation".to_string(),
            AgentModelSetting {
                model_config_id: Some(created.id.to_string()),
                model: Some("gpt-4".into()),
                effort: Some("high".into()),
            },
        );
        let saved = upsert_agent_models(&ctx.client, ctx.user_id, &ctx.config_root, models)
            .await
            .unwrap();
        let setting = saved.get("implementation").unwrap();
        assert_eq!(
            setting.model_config_id.as_deref(),
            Some(created.id.to_string().as_str())
        );

        // The agent_harness_configs row references the .rig.json file
        let cfg_dir = ProviderConfigDir::new(&ctx.config_root);
        assert!(cfg_dir
            .config_path(&format!("{}.rig.json", created.id))
            .exists());

        // A model not in the saved list is rejected
        let mut models = HashMap::new();
        models.insert(
            "review".to_string(),
            AgentModelSetting {
                model_config_id: Some(created.id.to_string()),
                model: Some("not-a-model".into()),
                effort: None,
            },
        );
        let result = upsert_agent_models(&ctx.client, ctx.user_id, &ctx.config_root, models).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not in provider"));
    }

    #[tokio::test]
    async fn test_upsert_agent_models_rejects_unknown_config() {
        let ctx = setup().await;
        let mut models = HashMap::new();
        models.insert(
            "implementation".to_string(),
            AgentModelSetting {
                model_config_id: Some(Uuid::new_v4().to_string()),
                model: Some("gpt-4".into()),
                effort: None,
            },
        );
        let saved = upsert_agent_models(&ctx.client, ctx.user_id, &ctx.config_root, models)
            .await
            .unwrap();
        let setting = saved.get("implementation").unwrap();
        // An unknown config id resolves to an empty ref (no file is written),
        // which is the pre-existing opencode flow's behavior.
        assert!(setting.model_config_id.is_none());
    }

    #[tokio::test]
    async fn test_rig_provider_api_key_masked_in_reads() {
        let ctx = setup().await;
        let created = create_rig_provider(
            &ctx.client,
            ctx.user_id,
            &ctx.config_root,
            sample_rig(crate::providers::rig_config::RigVendor::OpenAi),
        )
        .await
        .unwrap();
        assert_eq!(created.config.api_key.as_deref(), Some("••••-123"));
        let listed = list_rig_providers(&ctx.client, ctx.user_id).await.unwrap();
        let served = listed[0].config.api_key.as_deref().unwrap();
        assert!(served.contains("••••"));
        assert!(!served.contains("sk-123"));
        let fetched = get_rig_provider(&ctx.client, ctx.user_id, created.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched.config.api_key.as_deref(), Some("••••-123"));
    }

    #[tokio::test]
    async fn test_update_rig_provider_blank_key_keeps_stored() {
        let ctx = setup().await;
        let created = create_rig_provider(
            &ctx.client,
            ctx.user_id,
            &ctx.config_root,
            sample_rig(crate::providers::rig_config::RigVendor::OpenAi),
        )
        .await
        .unwrap();
        // The read response carries the masked key; submitting it unchanged
        // must preserve the stored secret rather than overwriting it.
        let mut cfg = created.config.clone();
        cfg.name = "renamed".into();
        update_rig_provider(&ctx.client, ctx.user_id, &ctx.config_root, created.id, cfg)
            .await
            .unwrap()
            .unwrap();
        let row = get_rig_provider_row(&ctx.client, ctx.user_id, created.id)
            .await
            .unwrap()
            .unwrap();
        let stored: RigProviderConfig = serde_json::from_str(&row.config_body).unwrap();
        assert_eq!(stored.api_key.as_deref(), Some("sk-123"));
    }

    #[tokio::test]
    async fn test_update_rig_provider_new_key_replaces() {
        let ctx = setup().await;
        let created = create_rig_provider(
            &ctx.client,
            ctx.user_id,
            &ctx.config_root,
            sample_rig(crate::providers::rig_config::RigVendor::OpenAi),
        )
        .await
        .unwrap();
        let mut cfg = created.config.clone();
        cfg.api_key = Some("sk-new-key".into());
        update_rig_provider(&ctx.client, ctx.user_id, &ctx.config_root, created.id, cfg)
            .await
            .unwrap()
            .unwrap();
        let row = get_rig_provider_row(&ctx.client, ctx.user_id, created.id)
            .await
            .unwrap()
            .unwrap();
        let stored: RigProviderConfig = serde_json::from_str(&row.config_body).unwrap();
        assert_eq!(stored.api_key.as_deref(), Some("sk-new-key"));
    }
}
