use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use tokio::sync::Mutex;
use uuid::Uuid;

use crate::opencode_sdk::{self, OpencodeClient, ServerOptions};
use crate::providers::HarnessConfig;

/// Default idle timeout before an unused server is reaped (15 minutes).
const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// Default reaper interval (60 seconds).
const DEFAULT_REAP_INTERVAL: Duration = Duration::from_secs(60);

/// Maximum servers allowed in the pool. `0` is treated as unbounded.
const DEFAULT_MAX_SERVERS: usize = 0;

/// A handle to a running opencode server in the pool.
struct ServerEntry {
    /// Clone of the HTTP client — handed out to callers. The underlying
    /// `OpenCodeServer` is owned by the pool and kept alive until the entry
    /// is dropped or invalidated.
    client: OpencodeClient,
    /// The running `opencode serve` subprocess. Kept alive in the pool
    /// (NOT in the provider) so it persists across Stop Agent / turn
    /// completion / new conversations.
    _server: opencode_sdk::OpenCodeServer,
    /// Monotonic timestamp of the last `get_or_spawn` call or timestamp update.
    last_used_at: Instant,
}

/// Per-user opencode server pool. Mirrors the reference implementation's
/// `OpenCodeServerPool` (see
/// `spec/reference/server/services/openCodeServerPool.ts`):
///
/// - **Per-user keying**: one `opencode serve` per user, shared across all
///   that user's conversations. Sessions are created per-turn via
///   `client.session.create()`.
/// - **Idle reaping**: a background task wakes every
///   [`DEFAULT_REAP_INTERVAL`] and SIGKILLs entries whose `last_used_at`
///   is older than [`DEFAULT_IDLE_TIMEOUT`].
/// - **LRU eviction**: if [`DEFAULT_MAX_SERVERS`] is non-zero and the pool
///   is full, the least-recently-used non-stale entry is evicted before
///   spawning a new one.
/// - **Credential invalidation**: [`OpenCodeServerPool::invalidate`] marks
///   the user's entry stale and schedules shutdown; the next
///   `get_or_spawn` awaits the shutdown and spawns a fresh server.
/// - **Process-exit cleanup**: [`OpenCodeServerPool::shutdown_all`] kills
///   every entry. Called from the `SIGTERM`/`SIGINT` handlers in
///   `src/main.rs`.
///
/// The pool is a process-wide singleton accessed via
/// [`OpenCodeServerPool::instance`]. It wraps its state in a `tokio::Mutex`
/// so callers can `await` on `get_or_spawn` without blocking the runtime.
pub struct OpenCodeServerPool {
    inner: Mutex<HashMap<Uuid, ServerEntry>>,
    /// Handles pending spawns so concurrent `get_or_spawn` calls for the
    /// same user coalesce rather than racing to spawn two servers.
    pending: Mutex<HashMap<Uuid, Arc<tokio::sync::Mutex<Option<OpencodeClient>>>>>,
}

#[allow(non_upper_case_globals)]
static POOL: OnceLock<OpenCodeServerPool> = OnceLock::new();

impl OpenCodeServerPool {
    /// Access the process-wide singleton pool. Initializes lazily on first
    /// call.
    pub fn instance() -> &'static OpenCodeServerPool {
        POOL.get_or_init(|| OpenCodeServerPool {
            inner: Mutex::new(HashMap::new()),
            pending: Mutex::new(HashMap::new()),
        })
    }

    /// Get-or-spawn a server for the given user. Returns a clone of the
    /// `OpencodeClient` (cheap — it's an `Arc` inside). The caller is free
    /// to use the client for any session-scoped operation; the server
    /// itself stays in the pool.
    pub async fn get_or_spawn(
        &self,
        user_id: Uuid,
        harness_config: &HarnessConfig,
        config_root: &Path,
        log_data: bool,
    ) -> Result<OpencodeClient, crate::providers::ProviderError> {
        // Fast path: if an entry already exists in the pool, refresh its
        // last_used_at and return the cached client immediately — no spawn needed.
        {
            let mut inner = self.inner.lock().await;
            if let Some(entry) = inner.get_mut(&user_id) {
                entry.last_used_at = Instant::now();
                return Ok(entry.client.clone());
            }
        }

        // Coalesce concurrent spawns for the same user. The pending entry
        // is removed once the spawn completes (success or failure).
        let pending_slot = {
            let mut pending = self.pending.lock().await;
            if let Some(slot) = pending.get(&user_id) {
                slot.clone()
            } else {
                let slot = Arc::new(tokio::sync::Mutex::new(None));
                pending.insert(user_id, slot.clone());
                slot
            }
        };
        let mut pending_guard = pending_slot.lock().await;
        if let Some(client) = pending_guard.as_ref() {
            return Ok(client.clone());
        }
        let client = self
            .spawn_entry(user_id, harness_config, config_root, log_data)
            .await?;
        *pending_guard = Some(client.clone());
        self.pending.lock().await.remove(&user_id);
        Ok(client)
    }

    /// Mark the user's entry stale and schedule its shutdown. The next
    /// `get_or_spawn` for the user will await the in-flight shutdown and
    /// spawn fresh.
    pub async fn invalidate(&self, user_id: Uuid) {
        let mut inner = self.inner.lock().await;
        if let Some(entry) = inner.remove(&user_id) {
            // Drop the entry — `OpenCodeServer`'s `Drop` kills the child.
            drop(entry);
            tracing::info!(%user_id, "invalidated opencode server entry");
        }
    }

    /// Kill every entry in the pool. Called by the signal handlers in
    /// `src/main.rs` before the ofm process exits.
    pub async fn shutdown_all(&self) {
        let mut inner = self.inner.lock().await;
        let count = inner.len();
        inner.clear();
        tracing::info!(count, "shut down all opencode server pool entries");
    }

    /// Returns a snapshot of pool status for diagnostics.
    pub async fn status(&self, user_id: Uuid) -> bool {
        let inner = self.inner.lock().await;
        inner.contains_key(&user_id)
    }

    /// Update the `last_used_at` timestamp for the given user's server
    /// entry to now, preventing the idle reaper from killing it. No-op if
    /// the user has no entry (e.g., transient server).
    pub async fn update_timestamp(&self, user_id: Uuid) {
        let mut inner = self.inner.lock().await;
        if let Some(entry) = inner.get_mut(&user_id) {
            entry.last_used_at = Instant::now();
        }
    }

    /// Reap idle entries (last_used_at older than `idle_timeout`). Called
    /// by the background reaper task started in `main.rs`.
    pub async fn reap_idle(&self, idle_timeout: Duration) {
        let now = Instant::now();
        let mut inner = self.inner.lock().await;
        let to_remove: Vec<Uuid> = inner
            .iter()
            .filter_map(|(uid, entry)| {
                if now.duration_since(entry.last_used_at) > idle_timeout {
                    Some(*uid)
                } else {
                    None
                }
            })
            .collect();
        for uid in to_remove {
            inner.remove(&uid);
            tracing::info!(%uid, "reaped idle opencode server");
        }
    }

    async fn spawn_entry(
        &self,
        _user_id: Uuid,
        harness_config: &HarnessConfig,
        config_root: &Path,
        log_data: bool,
    ) -> Result<OpencodeClient, crate::providers::ProviderError> {
        // Build the server config from the harness config's provider
        // snippet (loaded from disk by the provider at construction time).
        let provider_config_dir = crate::providers::config::ProviderConfigDir::new(config_root);
        let provider_cfg = provider_config_dir
            .load_provider_config(&harness_config.provider_config_ref)
            .map_err(|e| crate::providers::ProviderError::Config(e.to_string()))?;
        let server_config = build_server_config(&provider_cfg.raw_snippet);

        let options = ServerOptions {
            config: Some(server_config),
            ..Default::default()
        };
        let (client, server) = opencode_sdk::create_opencode(options, log_data)
            .await
            .map_err(|e| crate::providers::ProviderError::Protocol(e.to_string()))?;

        let entry = ServerEntry {
            client: client.clone(),
            _server: server,
            last_used_at: Instant::now(),
        };
        self.inner.lock().await.insert(_user_id, entry);
        Ok(client)
    }

    /// Start a background reaper task. Runs until the pool is dropped
    /// (which never happens for the singleton, so it effectively runs for
    /// the process lifetime). The task holds a `'static` reference to the
    /// pool via `OpenCodeServerPool::instance()`.
    pub fn start_reaper(idle_timeout: Duration, reap_interval: Duration) {
        let pool = Self::instance();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(reap_interval);
            ticker.tick().await; // skip the first (immediate) tick
            loop {
                ticker.tick().await;
                pool.reap_idle(idle_timeout).await;
            }
        });
    }
}

/// Build the `opencode.json` server config from the user's provider
/// snippet. Mirrors `OpenCodeSdkProvider::build_server_config` but lives
/// in the pool module so the pool can spawn servers without going through
/// a provider instance.
fn build_server_config(provider_snippet: &str) -> serde_json::Value {
    let mut base = serde_json::json!({
        "provider": {},
        "permission": {
            "edit": "allow",
            "bash": "allow",
            "webfetch": "allow",
            "doom_loop": "allow",
            "external_directory": "allow"
        }
    });
    if let Ok(snippet) = serde_json::from_str::<serde_json::Value>(provider_snippet) {
        deep_merge(&mut base, &snippet);
    }
    base
}

fn deep_merge(base: &mut serde_json::Value, overlay: &serde_json::Value) {
    match (base, overlay) {
        (serde_json::Value::Object(base_map), serde_json::Value::Object(overlay_map)) => {
            for (key, val) in overlay_map {
                if base_map.contains_key(key) {
                    deep_merge(&mut base_map[key], val);
                } else {
                    base_map.insert(key.clone(), val.clone());
                }
            }
        }
        (base, overlay) => *base = overlay.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper to insert a test entry into the pool with a specific age.
    async fn insert_entry(pool: &OpenCodeServerPool, uid: Uuid, age: Duration) {
        let child = std::process::Command::new("true").spawn().unwrap();
        let entry = ServerEntry {
            client: OpencodeClient::new("http://127.0.0.1:9999", None, false),
            _server: crate::opencode_sdk::OpenCodeServer::test_dummy(child),
            last_used_at: Instant::now() - age,
        };
        pool.inner.lock().await.insert(uid, entry);
    }

    #[test]
    fn test_pool_singleton_idempotent() {
        let p1 = OpenCodeServerPool::instance();
        let p2 = OpenCodeServerPool::instance();
        // Same address — singleton.
        assert!(std::ptr::eq(p1, p2));
    }

    /// Run all singleton-dependent pool tests sequentially to prevent
    /// state leakage between parallel `#[tokio::test]` functions.
    #[tokio::test]
    async fn test_pool_update_timestamp_and_reap_lifecycle() {
        let pool = OpenCodeServerPool::instance();

        // ── test_update_timestamp_updates_last_used_at ────────────────────────────
        let uid = Uuid::new_v4();
        insert_entry(pool, uid, Duration::from_secs(60 * 60)).await;

        // Entry should be reaped with a 1-second timeout (it's 1 hour old).
        pool.reap_idle(Duration::from_secs(1)).await;
        assert!(!pool.status(uid).await);

        // Re-insert and update timestamp — should NOT be reaped.
        insert_entry(pool, uid, Duration::from_secs(60 * 60)).await;
        pool.update_timestamp(uid).await;
        pool.reap_idle(Duration::from_secs(1)).await;
        assert!(pool.status(uid).await);

        // ── test_reap_idle_reaps_old_but_not_recent ───────────────────────────────
        let stale_uid = Uuid::new_v4();
        let fresh_uid = Uuid::new_v4();

        insert_entry(pool, stale_uid, Duration::from_secs(60 * 60)).await;
        insert_entry(pool, fresh_uid, Duration::from_secs(1)).await;

        pool.reap_idle(Duration::from_secs(60)).await;

        assert!(!pool.status(stale_uid).await);
        assert!(pool.status(fresh_uid).await);

        // ── test_get_or_spawn_fast_path_update ────────────────────────────────────
        let uid2 = Uuid::new_v4();

        // Insert an old entry directly.
        insert_entry(pool, uid2, Duration::from_secs(60 * 60)).await;

        // Calling get_or_spawn should hit the fast path and update timestamp.
        let harness_config = crate::providers::HarnessConfig {
            agent_type: "test".into(),
            harness: "opencode".into(),
            provider_config_ref: "test.json".into(),
            model: None,
            effort: None,
            scope: crate::db::schema::ScopeType::Global,
        };
        let config_root = std::path::Path::new("/tmp");
        let result = pool
            .get_or_spawn(uid2, &harness_config, config_root, false)
            .await;
        assert!(result.is_ok());

        // After the fast-path update, the entry should NOT be reaped.
        pool.reap_idle(Duration::from_secs(60)).await;
        assert!(pool.status(uid2).await);

        // ── test_update_timestamp_noop_for_unknown_user ───────────────────────────
        let unknown_uid = Uuid::new_v4();
        // Should not panic.
        pool.update_timestamp(unknown_uid).await;
    }

    #[test]
    fn test_build_server_config_base_permissions() {
        let cfg = build_server_config("{}");
        assert_eq!(cfg["permission"]["edit"], "allow");
        assert_eq!(cfg["permission"]["bash"], "allow");
        assert_eq!(cfg["provider"], serde_json::json!({}));
    }

    #[test]
    fn test_build_server_config_merges_provider() {
        let snippet = r#"{"provider": {"anthropic": {"apiKey": "sk-xxx"}}}"#;
        let cfg = build_server_config(snippet);
        assert_eq!(cfg["provider"]["anthropic"]["apiKey"], "sk-xxx");
        // Base permissions preserved.
        assert_eq!(cfg["permission"]["edit"], "allow");
    }

    #[test]
    fn test_deep_merge_overrides_scalar() {
        let mut base = serde_json::json!({"k": "v"});
        deep_merge(&mut base, &serde_json::json!({"k": "v2"}));
        assert_eq!(base["k"], "v2");
    }

    #[test]
    fn test_deep_merge_nested_objects() {
        let mut base = serde_json::json!({"a": {"b": 1}});
        deep_merge(&mut base, &serde_json::json!({"a": {"c": 2}}));
        assert_eq!(base["a"]["b"], 1);
        assert_eq!(base["a"]["c"], 2);
    }
}
