//! System Status & Health monitoring.
//!
//! Two checks feed the rolling `system_health_entry` table:
//!
//! - **Dependency check** (`dependency_check`): probes a code-based list of
//!   binaries in `PATH` (`src/services/system_health.rs` `BINS`). A subset is
//!   *required* for startup (`bin_required`); a missing required bin aborts
//!   `main` with an exit-1 report.
//! - **Live system health** (`live_health_check`): snapshots the sub-systems
//!   OFM manages — the opencode server pool, rauthy (or the external OIDC
//!   provider), the embedded hiqlite cluster, and `gh` auth state.
//!
//! Rows are ephemeral/rolling: every refresh inserts fresh rows and prunes to
//! the newest [`MAX_ROWS_PER_PRUNE`]. Latest-state-per-resource is the
//! `ORDER BY id DESC`-deduped view from [`latest_report`]; agents can consume
//! the time series via [`history_report`] for mermaid charts.
//!
//! The report is delivered three ways: a markdown console report at startup,
//! a markdown+JSON page (`/webapp/system`), and JSON + a WS `system_status`
//! event (`/api/system/status`, the navbar badge).
//!
//! The `system-status` OAuth-scope group gates **agent-session injection** of
//! this data only (see [`user_can_use_system_health`]); the page itself is for
//! any authenticated user.

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;

use hiqlite::Client;
use serde_json::{json, Value};

use crate::auth::AuthUser;
use crate::config::OfmConfig;
use crate::db::schema::SystemHealthEntryDb;
use crate::services::groups::GroupError;
use uuid::Uuid;

/// How often the background monitor re-probes live health and broadcasts a
/// `system_status` event (milliseconds).
pub const DEFAULT_REFRESH_INTERVAL_MS: u64 = 30_000;

/// Retention cap: after every refresh the table is pruned to the newest rows.
pub const MAX_ROWS_PER_PRUNE: i64 = 500;

/// Upper bound for the agent-facing history endpoint (rows per resource).
pub const HISTORY_LIMIT: i64 = 1000;

/// Process health meter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthStatus {
    Ok,
    Warn,
    Missing,
    Error,
    Unknown,
}

impl HealthStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            HealthStatus::Ok => "ok",
            HealthStatus::Warn => "warn",
            HealthStatus::Missing => "missing",
            HealthStatus::Error => "error",
            HealthStatus::Unknown => "unknown",
        }
    }

    /// Unicode meter symbols used in markdown renderings.
    pub fn icon(&self) -> &'static str {
        match self {
            HealthStatus::Ok => "✔",
            HealthStatus::Warn => "▲",
            HealthStatus::Missing => "✖",
            HealthStatus::Error => "✘",
            HealthStatus::Unknown => "–",
        }
    }
}

impl FromStr for HealthStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ok" => Ok(HealthStatus::Ok),
            "warn" => Ok(HealthStatus::Warn),
            "missing" => Ok(HealthStatus::Missing),
            "error" => Ok(HealthStatus::Error),
            "unknown" => Ok(HealthStatus::Unknown),
            other => Err(format!("invalid health status: '{other}'")),
        }
    }
}

/// One probe result, persisted as a `system_health_entry` row by
/// [`refresh_entries`].
#[derive(Debug, Clone)]
pub struct HealthEntry {
    pub category: &'static str,
    pub resource: String,
    pub status: HealthStatus,
    pub detail: String,
    pub metadata: Value,
}

fn utc_now() -> String {
    chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

fn always(_cfg: &OfmConfig) -> bool {
    true
}

fn iff_rauthy(cfg: &OfmConfig) -> bool {
    cfg.rauthy_enabled
}

fn never(_cfg: &OfmConfig) -> bool {
    false
}

/// A single code-defined binary probe.
pub struct BinSpec {
    /// Stable key used as `resource` suffix (`bin:{key}`).
    key: &'static str,
    /// Binary to look up in `PATH`.
    bin: &'static str,
    /// Human-readable name for detail strings.
    name: &'static str,
    /// Whether a missing probe is a startup blocker for the given config.
    required: fn(&OfmConfig) -> bool,
    /// CLI flags used to query the version (e.g. `--version`).
    version_flags: &'static [&'static str],
    /// Extra informational binaries probed alongside the primary one, each
    /// reported as its own `bin:{extra}` entry (e.g. `git-lfs` for `git`).
    extra_bins: &'static [&'static str],
    /// Fallback binary tried when the primary is absent (e.g. `sh` for `bash`).
    fallback: Option<&'static str>,
}

impl BinSpec {
    const fn new(
        key: &'static str,
        bin: &'static str,
        name: &'static str,
        required: fn(&OfmConfig) -> bool,
        version_flags: &'static [&'static str],
        extra_bins: &'static [&'static str],
    ) -> Self {
        Self {
            key,
            bin,
            name,
            required,
            version_flags,
            extra_bins,
            fallback: None,
        }
    }

    const fn fallback(mut self, fallback: &'static str) -> Self {
        self.fallback = Some(fallback);
        self
    }
}

/// Code-based list of binaries the dependency check probes. `required`
/// predicates drive the startup crash decision (`docker` is only required when
/// rauthy is enabled; `gh`, `rustup`/`cargo`, `playwright-cli` and `rtk` are
/// reported-but-non-fatal when missing).
const BINS: &[BinSpec] = &[
    BinSpec::new("git", "git", "git", always, &["--version"], &["git-lfs"]),
    BinSpec::new(
        "opencode",
        "opencode",
        "opencode",
        always,
        &["--version"],
        &[],
    ),
    BinSpec::new("bash", "bash", "bash/sh", always, &["--version"], &[]).fallback("sh"),
    BinSpec::new(
        "npm",
        "npm",
        "npm",
        always,
        &["--version"],
        &["npx", "node"],
    ),
    BinSpec::new(
        "docker",
        "docker",
        "docker",
        iff_rauthy,
        &["--version"],
        &[],
    ),
    BinSpec::new("gh", "gh", "gh", never, &["--version"], &[]),
    BinSpec::new(
        "rustup",
        "rustup",
        "rustup",
        never,
        &["--version"],
        &["cargo"],
    ),
    BinSpec::new(
        "playwright-cli",
        "playwright-cli",
        "playwright-cli",
        never,
        &["--version"],
        &[],
    ),
    BinSpec::new("rtk", "rtk", "rtk", never, &["--version"], &[]),
];

// ── Tool detection (pure, unit-testable) ─────────────────────────────────────

/// Find `bin` in `PATH` (split on `:` on unix, `;` anywhere) and return the
/// first candidate that exists and is executable. Does not shell out to
/// `which`.
pub fn find_in_path(bin: &str) -> Option<PathBuf> {
    let path = std::env::var("PATH").unwrap_or_default();
    find_in_path_with_path(bin, &path)
}

fn find_in_path_with_path(bin: &str, path_var: &str) -> Option<PathBuf> {
    for dir in path_var.split([':', ';']).filter(|d| !d.is_empty()) {
        let candidate = Path::new(dir).join(bin);
        if candidate.is_file() && is_executable(&candidate) {
            return Some(candidate);
        }
    }
    None
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.is_file() && (m.permissions().mode() & 0o111) != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

/// Run `<bin> <flags>` and return the first trimmed stdout line, `None` on
/// spawn failure or non-zero exit.
fn read_tool_version(bin_path: &Path, flags: &[&str]) -> Option<String> {
    let output = std::process::Command::new(bin_path)
        .args(flags)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout.lines().next().unwrap_or("").trim();
    parse_version(line, &[])
}

/// Strip common prefixes and `tool/version` shapes, keeping the version
/// token. `"git version 2.43.0"` → `"2.43.0"`, `"npx/10.8.2"` → `"10.8.2"`,
/// junk → `None`.
pub fn parse_version(output: &str, prefix_strip: &[&str]) -> Option<String> {
    let mut s = output.trim();
    if s.is_empty() {
        return None;
    }
    for p in prefix_strip {
        let p = p.trim();
        if !p.is_empty() && s.starts_with(p) {
            s = s[p.len()..].trim_start();
            break;
        }
    }
    // `tool/1.2.3` style output (e.g. `npx/10.8.2`).
    if let Some((_, rest)) = s.rsplit_once('/') {
        let rest = rest.trim();
        if rest
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit() || c == 'v')
        {
            s = rest;
        }
    }
    // First whitespace-delimited token (e.g. `Docker version 27.3.1, build`).
    if let Some((tok, _)) = s.split_once(|c: char| c.is_whitespace()) {
        s = tok;
    }
    let s = s.trim_matches(|c: char| c == ',' || c == '.' || c == ' ');
    let starts_ok = s
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_digit() || c == 'v');
    if s.is_empty() || !starts_ok {
        None
    } else {
        Some(s.to_string())
    }
}

/// Heuristic install-method classification from a binary's path.
pub fn install_method(path: &Path) -> &'static str {
    let s = path.to_string_lossy();
    if s.contains("/.cargo/bin/") {
        "cargo"
    } else if s.contains("npm-global") || s.contains("nvm") || s.contains("node_modules/.bin") {
        "npm"
    } else if s.starts_with("/usr/bin") || s.starts_with("/bin") || s.starts_with("/usr/local/bin")
    {
        "system"
    } else {
        "other"
    }
}

/// Directory names under the Playwright browser install base (`~/.cache/
/// ms-playwright`, or `PLAYWRIGHT_BROWSERS_PATH` when set).
pub fn playwright_browsers() -> Vec<String> {
    let base = std::env::var("PLAYWRIGHT_BROWSERS_PATH")
        .ok()
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|h| Path::new(&h).join(".cache").join("ms-playwright"))
        });
    let Some(base) = base else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&base) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                if let Some(name) = entry.file_name().to_str() {
                    out.push(name.to_string());
                }
            }
        }
    }
    out.sort();
    out
}

// ── Dependency check ────────────────────────────────────────────────────────

/// Whether `resource` (e.g. `bin:docker`) is a startup-required dependency for
/// the given config. Used by `main` to decide the early crash.
pub fn bin_required(cfg: &OfmConfig, resource: &str) -> bool {
    let key = resource.strip_prefix("bin:").unwrap_or(resource);
    BINS.iter().any(|s| s.key == key && (s.required)(cfg))
}

fn probe_bin(spec: &BinSpec, path_var: &str) -> HealthEntry {
    let resource = format!("bin:{}", spec.key);
    let mut meta = serde_json::Map::new();

    let mut found = find_in_path_with_path(spec.bin, path_var);
    if found.is_none() {
        if let Some(fb) = spec.fallback {
            found = find_in_path_with_path(fb, path_var);
        }
    }

    if let Some(p) = found {
        if let Some(v) = read_tool_version(&p, spec.version_flags) {
            meta.insert("version".into(), json!(v));
        }
        meta.insert("path".into(), json!(p.to_string_lossy()));
        meta.insert("install_method".into(), json!(install_method(&p)));
        meta.insert("last_interaction".into(), json!(utc_now()));
        HealthEntry {
            category: "dependency",
            resource,
            status: HealthStatus::Ok,
            detail: format!("{} found at {}", spec.name, p.display()),
            metadata: Value::Object(meta),
        }
    } else {
        HealthEntry {
            category: "dependency",
            resource,
            status: HealthStatus::Missing,
            detail: format!("{} not found in PATH", spec.name),
            metadata: Value::Object(meta),
        }
    }
}

fn probe_extra_bin(bin: &str, path_var: &str) -> HealthEntry {
    let resource = format!("bin:{bin}");
    let mut meta = serde_json::Map::new();
    if let Some(p) = find_in_path_with_path(bin, path_var) {
        if let Some(v) = read_tool_version(&p, &["--version"]) {
            meta.insert("version".into(), json!(v));
        }
        meta.insert("path".into(), json!(p.to_string_lossy()));
        meta.insert("install_method".into(), json!(install_method(&p)));
        meta.insert("last_interaction".into(), json!(utc_now()));
        HealthEntry {
            category: "dependency",
            resource,
            status: HealthStatus::Ok,
            detail: format!("{bin} found at {}", p.display()),
            metadata: Value::Object(meta),
        }
    } else {
        HealthEntry {
            category: "dependency",
            resource,
            status: HealthStatus::Missing,
            detail: format!("{bin} not found in PATH"),
            metadata: Value::Object(meta),
        }
    }
}

/// Probe every bin in the code-based list (blocking subprocess calls run on a
/// blocking thread per AGENTS.md).
pub async fn dependency_check(_cfg: &OfmConfig) -> Vec<HealthEntry> {
    let path_var = std::env::var("PATH").unwrap_or_default();
    let mut entries = tokio::task::spawn_blocking(move || {
        let mut entries = Vec::new();
        for spec in BINS {
            entries.push(probe_bin(spec, &path_var));
            for extra in spec.extra_bins {
                entries.push(probe_extra_bin(extra, &path_var));
            }
        }
        entries
    })
    .await
    .unwrap_or_default();

    // Attach the Playwright browser install list to the playwright-cli entry.
    if let Some(pw) = entries
        .iter_mut()
        .find(|e| e.resource == "bin:playwright-cli")
    {
        let browsers = tokio::task::spawn_blocking(playwright_browsers)
            .await
            .unwrap_or_default();
        if !browsers.is_empty() {
            if let Value::Object(map) = &mut pw.metadata {
                map.insert("browsers".into(), json!(browsers));
            }
        }
    }
    entries
}

// ── Live system health ───────────────────────────────────────────────────────

/// Actual rauthy listen port once the embedded instance is up (the configured
/// `cfg.rauthy_port` may be `0` = random). Recorded by `main` after
/// `start_rauthy`; the background monitor reads it without borrowing
/// `AppState`.
static RAUTHY_PORT: std::sync::OnceLock<u16> = std::sync::OnceLock::new();

pub fn set_rauthy_port(port: u16) {
    let _ = RAUTHY_PORT.set(port);
}

/// Resident set size of `pid` in KB, read from `/proc/{pid}/statm` (linux;
/// `None` elsewhere).
pub fn process_rss_kb(pid: u32) -> Option<u64> {
    #[cfg(unix)]
    {
        let content = std::fs::read_to_string(format!("/proc/{pid}/statm")).ok()?;
        let mut fields = content.split_whitespace();
        let _size = fields.next()?;
        let resident_pages = fields.next()?.parse::<u64>().ok()?;
        // 4096-byte pages → KB.
        Some(resident_pages * 4)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        None
    }
}

/// PID of the named docker container, via `docker inspect`.
fn container_pid(container: &str) -> Option<u32> {
    let out = std::process::Command::new("docker")
        .args(["inspect", "--format", "{{.State.Pid}}", container])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse::<u32>()
        .ok()
        .filter(|pid| *pid > 0)
}

/// Recursive size (bytes) and newest-file mtime of `dir` — used to approximate
/// the hiqlite footprint and its last filesystem flush.
fn dir_footprint_stats(dir: &Path) -> (u64, Option<String>) {
    let mut total = 0u64;
    let mut newest_millis: Option<u64> = None;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(p) = stack.pop() {
        let Ok(meta) = std::fs::symlink_metadata(&p) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            if let Ok(rd) = std::fs::read_dir(&p) {
                for entry in rd.flatten() {
                    stack.push(entry.path());
                }
            }
        } else {
            total = total.saturating_add(meta.len());
            if let Ok(modified) = meta.modified() {
                if let Ok(dur) = modified.duration_since(std::time::UNIX_EPOCH) {
                    let millis = dur.as_millis() as u64;
                    newest_millis = Some(newest_millis.map_or(millis, |n| n.max(millis)));
                }
            }
        }
    }
    let last_flush = newest_millis.map(|millis| {
        chrono::DateTime::<chrono::Utc>::from_timestamp_millis(millis as i64)
            .map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_default()
    });
    (total, last_flush)
}

/// Full live snapshot. Runs once at startup and again on every monitor tick.
/// Takes only `&Client` + `&OfmConfig` so the periodic task never borrows
/// `AppState`.
pub async fn live_health_check(client: &Client, cfg: &OfmConfig) -> Vec<HealthEntry> {
    let mut entries = Vec::new();
    entries.push(live_opencode_pool().await);
    entries.push(live_rauthy_or_oauth(cfg).await);
    entries.push(live_hiqlite(client, cfg).await);
    entries.push(live_gh(cfg).await);
    entries
}

async fn live_opencode_pool() -> HealthEntry {
    let resource = "live:opencode-pool".to_string();
    let mut meta = serde_json::Map::new();
    let entries = crate::opencode_sdk::pool::OpenCodeServerPool::instance()
        .monitor_snapshot()
        .await;
    let mut by_status: std::collections::BTreeMap<String, usize> = Default::default();
    let servers: Vec<Value> = entries
        .iter()
        .map(|e| {
            let status = if e.pid.is_some() { "running" } else { "idle" };
            *by_status.entry(status.to_string()).or_default() += 1;
            json!({
                "user_id": e.user_id.to_string(),
                "pid": e.pid,
                "port": e.port,
                "ram_kb": e.rss_kb,
            })
        })
        .collect();
    meta.insert("count".into(), json!(entries.len()));
    meta.insert("by_status".into(), json!(by_status));
    meta.insert("servers".into(), json!(servers));
    meta.insert("last_interaction".into(), json!(utc_now()));
    let detail = format!("{} pooled opencode server(s)", entries.len());
    HealthEntry {
        category: "live",
        resource,
        status: HealthStatus::Ok,
        detail,
        metadata: Value::Object(meta),
    }
}

async fn live_rauthy_or_oauth(cfg: &OfmConfig) -> HealthEntry {
    let mut meta = serde_json::Map::new();
    if cfg.rauthy_enabled {
        let resource = "live:rauthy".to_string();
        let port = RAUTHY_PORT.get().copied().unwrap_or(cfg.rauthy_port);
        if port == 0 {
            return HealthEntry {
                category: "live",
                resource,
                status: HealthStatus::Unknown,
                detail: "rauthy enabled but no listen port known yet".into(),
                metadata: Value::Object(meta),
            };
        }
        let url = format!("http://127.0.0.1:{port}/health");
        let start = std::time::Instant::now();
        let result = reqwest::Client::new()
            .get(&url)
            .timeout(Duration::from_secs(3))
            .send()
            .await;
        let latency_ms = start.elapsed().as_millis() as u64;
        meta.insert("latency_ms".into(), json!(latency_ms));
        match result {
            Ok(resp) => {
                let http = resp.status().as_u16();
                meta.insert("http_status".into(), json!(http));
                meta.insert(
                    "container".into(),
                    json!(crate::rauthy::container_name(&cfg.footprint)),
                );
                if let Some(pid) = container_pid(&crate::rauthy::container_name(&cfg.footprint)) {
                    meta.insert("pid".into(), json!(pid));
                    if let Some(rss) = process_rss_kb(pid) {
                        meta.insert("ram_kb".into(), json!(rss));
                    }
                }
                meta.insert("last_interaction".into(), json!(utc_now()));
                let status = if resp.status().is_success() {
                    HealthStatus::Ok
                } else {
                    HealthStatus::Warn
                };
                HealthEntry {
                    category: "live",
                    resource,
                    status,
                    detail: format!("rauthy health {url} → HTTP {http}"),
                    metadata: Value::Object(meta),
                }
            }
            Err(e) => HealthEntry {
                category: "live",
                resource,
                status: HealthStatus::Error,
                detail: format!("rauthy health probe failed: {e}"),
                metadata: Value::Object(meta),
            },
        }
    } else if let Some(issuer) = &cfg.oidc_issuer_url {
        let resource = "live:oauth".to_string();
        let discovery = format!(
            "{}/.well-known/openid-configuration",
            issuer.trim_end_matches('/')
        );
        let start = std::time::Instant::now();
        let result = reqwest::Client::new()
            .get(&discovery)
            .timeout(Duration::from_secs(3))
            .send()
            .await;
        let latency_ms = start.elapsed().as_millis() as u64;
        meta.insert("latency_ms".into(), json!(latency_ms));
        match result {
            Ok(resp) => {
                let http = resp.status().as_u16();
                meta.insert("http_status".into(), json!(http));
                meta.insert("last_interaction".into(), json!(utc_now()));
                let status = if resp.status().is_success() {
                    HealthStatus::Ok
                } else {
                    HealthStatus::Warn
                };
                HealthEntry {
                    category: "live",
                    resource,
                    status,
                    detail: format!("OIDC discovery {discovery} → HTTP {http}"),
                    metadata: Value::Object(meta),
                }
            }
            Err(e) => HealthEntry {
                category: "live",
                resource,
                status: HealthStatus::Error,
                detail: format!("OIDC discovery probe failed: {e}"),
                metadata: Value::Object(meta),
            },
        }
    } else {
        HealthEntry {
            category: "live",
            resource: "live:rauthy".to_string(),
            status: HealthStatus::Unknown,
            detail: "rauthy disabled; no external OIDC provider configured".into(),
            metadata: Value::Object(meta),
        }
    }
}

async fn live_hiqlite(client: &Client, cfg: &OfmConfig) -> HealthEntry {
    let resource = "live:hiqlite".to_string();
    let mut meta = serde_json::Map::new();
    let mut status = HealthStatus::Ok;
    let mut detail = String::new();

    let healthy = client.is_healthy_db().await.is_ok();
    let leader = client.is_leader_db().await;
    meta.insert("healthy".into(), json!(healthy));
    meta.insert("leader".into(), json!(leader));
    if !healthy {
        status = HealthStatus::Error;
        detail = "hiqlite cluster unhealthy".to_string();
    }

    match client.metrics_db().await {
        Ok(metrics) => {
            let last_applied = metrics
                .last_applied
                .as_ref()
                .map(|log_id| log_id.index)
                .unwrap_or(0);
            let current_term = metrics.current_term;
            meta.insert("current_term".into(), json!(current_term));
            meta.insert("last_applied".into(), json!(last_applied));
            meta.insert(
                "metrics".into(),
                serde_json::to_value(&metrics).unwrap_or(json!({})),
            );
        }
        Err(e) => {
            if status == HealthStatus::Ok {
                status = HealthStatus::Warn;
                detail = format!("hiqlite metrics unavailable: {e}");
            } else {
                detail = format!("{detail}; metrics unavailable: {e}");
            }
            meta.insert("metrics_error".into(), json!(e.to_string()));
        }
    }

    // Footprint size + last-flush approximation (mtime of the newest file).
    let data_dir = Path::new(&cfg.data_dir);
    let (size_bytes, last_flush) = dir_footprint_stats(data_dir);
    meta.insert("footprint_bytes".into(), json!(size_bytes));
    if let Some(last_flush) = last_flush {
        meta.insert("last_flush".into(), json!(last_flush));
    }

    if detail.is_empty() {
        detail = format!("hiqlite cluster healthy={healthy} leader={leader}");
    }
    meta.insert("last_interaction".into(), json!(utc_now()));
    HealthEntry {
        category: "live",
        resource,
        status,
        detail,
        metadata: Value::Object(meta),
    }
}

async fn live_gh(cfg: &OfmConfig) -> HealthEntry {
    let resource = "live:gh".to_string();
    let mut meta = serde_json::Map::new();
    let _ = cfg;
    if find_in_path("gh").is_none() {
        return HealthEntry {
            category: "live",
            resource,
            status: HealthStatus::Unknown,
            detail: "gh not installed".into(),
            metadata: Value::Object(meta),
        };
    }
    let output = tokio::time::timeout(
        Duration::from_secs(5),
        tokio::process::Command::new("gh")
            .args(["api", "user"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output(),
    )
    .await;
    match output {
        Ok(Ok(out)) if out.status.success() => {
            if let Ok(v) = serde_json::from_slice::<Value>(&out.stdout) {
                if let Some(login) = v.get("login").and_then(|l| l.as_str()) {
                    meta.insert("login".into(), json!(login));
                }
                if let Some(name) = v.get("name").and_then(|n| n.as_str()) {
                    meta.insert("name".into(), json!(name));
                }
            }
            meta.insert("last_interaction".into(), json!(utc_now()));
            HealthEntry {
                category: "live",
                resource,
                status: HealthStatus::Ok,
                detail: "gh authenticated".into(),
                metadata: Value::Object(meta),
            }
        }
        Ok(Ok(out)) => {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            HealthEntry {
                category: "live",
                resource,
                status: HealthStatus::Error,
                detail: format!("gh api user failed: {stderr}"),
                metadata: Value::Object(meta),
            }
        }
        Ok(Err(e)) => HealthEntry {
            category: "live",
            resource,
            status: HealthStatus::Error,
            detail: format!("gh spawn failed: {e}"),
            metadata: Value::Object(meta),
        },
        Err(_) => HealthEntry {
            category: "live",
            resource,
            status: HealthStatus::Warn,
            detail: "gh api user timed out".into(),
            metadata: Value::Object(meta),
        },
    }
}

// ── DB persistence ───────────────────────────────────────────────────────────

/// Insert one row per entry, then prune to the newest [`MAX_ROWS_PER_PRUNE`].
pub async fn refresh_entries(
    client: &Client,
    entries: &[HealthEntry],
) -> Result<usize, Box<dyn std::error::Error>> {
    let now = utc_now();
    for e in entries {
        client
            .execute(
                "INSERT INTO system_health_entry (category, resource, status, detail, metadata, created_at) \
                 VALUES ($1, $2, $3, $4, $5, $6)",
                hiqlite::params!(
                    e.category,
                    e.resource.clone(),
                    e.status.as_str(),
                    e.detail.clone(),
                    e.metadata.to_string(),
                    &now
                ),
            )
            .await?;
    }
    let prune_sql = format!(
        "DELETE FROM system_health_entry WHERE id NOT IN \
         (SELECT id FROM system_health_entry ORDER BY id DESC LIMIT {MAX_ROWS_PER_PRUNE})"
    );
    client.execute(prune_sql, hiqlite::params!()).await?;
    Ok(entries.len())
}

/// Latest row per `resource` (newest insert per resource), ordered by
/// resource. This is the "current state" view for the page/JSON/markdown.
pub async fn latest_report(client: &Client) -> Result<Vec<SystemHealthEntryDb>, hiqlite::Error> {
    client
        .query_map::<SystemHealthEntryDb, _>(
            "SELECT * FROM system_health_entry \
             WHERE id IN (SELECT MAX(id) FROM system_health_entry GROUP BY resource) \
             ORDER BY resource",
            hiqlite::params!(),
        )
        .await
}

/// Recent rolling rows (newest first) for agent/mermaid consumption. `limit`
/// is clamped to [`HISTORY_LIMIT`].
pub async fn history_report(
    client: &Client,
    resource: Option<&str>,
    limit: i64,
) -> Result<Vec<SystemHealthEntryDb>, hiqlite::Error> {
    let limit = limit.clamp(1, HISTORY_LIMIT);
    match resource {
        Some(r) => {
            let sql = format!(
                "SELECT * FROM system_health_entry WHERE resource = $1 \
                 ORDER BY id DESC LIMIT {limit}"
            );
            client
                .query_map::<SystemHealthEntryDb, _>(sql, hiqlite::params!(r))
                .await
        }
        None => {
            let sql = format!("SELECT * FROM system_health_entry ORDER BY id DESC LIMIT {limit}");
            client
                .query_map::<SystemHealthEntryDb, _>(sql, hiqlite::params!())
                .await
        }
    }
}

// ── Renderers ────────────────────────────────────────────────────────────────

fn parsed_meta(e: &SystemHealthEntryDb) -> Value {
    serde_json::from_str(&e.metadata).unwrap_or_else(|_| json!({}))
}

fn escape_pipe(s: &str) -> String {
    s.replace('|', "\\|")
}

/// Render a `| status | resource | version | path | pid | ram | last interaction |`
/// table for the given rows.
fn render_status_table(entries: &[SystemHealthEntryDb]) -> String {
    let mut s = String::new();
    s.push_str("| status | resource | detail | version | path | pid | ram | last interaction |\n");
    s.push_str("|--------|----------|--------|---------|------|-----|-----|------------------|\n");
    for e in entries {
        let meta = parsed_meta(e);
        let version = meta.get("version").and_then(|v| v.as_str()).unwrap_or("—");
        let path = meta.get("path").and_then(|v| v.as_str()).unwrap_or("—");
        let pid = meta
            .get("pid")
            .and_then(|v| v.as_i64())
            .map(|p| p.to_string())
            .unwrap_or_else(|| "—".into());
        let ram = meta
            .get("ram_kb")
            .and_then(|v| v.as_i64())
            .map(|k| format!("{k} KB"))
            .unwrap_or_else(|| "—".into());
        let last = meta
            .get("last_interaction")
            .and_then(|v| v.as_str())
            .unwrap_or("—");
        s.push_str(&format!(
            "| {} | {} | {} | {version} | {path} | {pid} | {ram} | {last} |\n",
            e.status,
            e.resource,
            escape_pipe(&e.detail)
        ));
    }
    s
}

/// Markdown report with `## Dependency Check` and `## Live System Health`
/// sections: per-line `- {icon} **{resource}** — {status} — {detail}` bullets
/// plus the detail table. Reused for the startup console report and the page.
pub fn render_markdown(entries: &[SystemHealthEntryDb]) -> String {
    let deps: Vec<SystemHealthEntryDb> = entries
        .iter()
        .filter(|e| e.category == "dependency")
        .cloned()
        .collect();
    let live: Vec<SystemHealthEntryDb> = entries
        .iter()
        .filter(|e| e.category == "live")
        .cloned()
        .collect();

    let mut s = String::new();
    s.push_str("## Dependency Check\n\n");
    s.push_str(&render_bullets(&deps));
    s.push('\n');
    s.push_str(&render_status_table(&deps));
    s.push_str("\n## Live System Health\n\n");
    s.push_str(&render_bullets(&live));
    s.push('\n');
    s.push_str(&render_status_table(&live));
    s
}

fn render_bullets(entries: &[SystemHealthEntryDb]) -> String {
    let mut s = String::new();
    for e in entries {
        let status = HealthStatus::from_str(&e.status)
            .map(|hs| hs.as_str())
            .unwrap_or("unknown");
        let icon = HealthStatus::from_str(&e.status)
            .map(|hs| hs.icon())
            .unwrap_or("–");
        s.push_str(&format!(
            "- {icon} **{}** — {status} — {}\n",
            e.resource,
            e.detail.replace('\n', " ")
        ));
    }
    s
}

/// Dependency-only bullet report for the early startup crash path (takes the
/// in-memory probe results, not DB rows).
pub fn render_markdown_deps_only(entries: &[HealthEntry]) -> String {
    let mut s = String::from("## Dependency Check\n\n");
    for e in entries {
        s.push_str(&format!(
            "- {} **{}** — {} — {}\n",
            e.status.icon(),
            e.resource,
            e.status.as_str(),
            e.detail
        ));
    }
    s
}

/// JSON report: `generated_at`, `running_services` (live entries with status
/// `ok`), and the full `entries` array with parsed metadata.
pub fn render_json(entries: &[SystemHealthEntryDb]) -> Value {
    let running_services = entries
        .iter()
        .filter(|e| e.category == "live" && e.status == "ok")
        .count();
    let entries_json: Vec<Value> = entries
        .iter()
        .map(|e| {
            json!({
                "category": e.category,
                "resource": e.resource,
                "status": e.status,
                "detail": e.detail,
                "metadata": parsed_meta(e),
                "created_at": e.created_at,
            })
        })
        .collect();
    json!({
        "generated_at": utc_now(),
        "running_services": running_services,
        "entries": entries_json,
    })
}

// ── Capability + agent context ───────────────────────────────────────────────

/// Whether `user` may use System Status & Health data in agent sessions.
/// Admins hold the capability implicitly; everyone else needs read-only
/// membership of the seeded `system-status` OAuth-scope group (granted by
/// having `system-status` in `users.scopes`).
pub async fn user_can_use_system_health(
    client: &Client,
    user: &AuthUser,
) -> Result<bool, GroupError> {
    let group_id = {
        // Scoped so the borrowed `Row` is dropped before the `.await` below
        // (hiqlite rows are not `Send`, so holding one across an await makes
        // the future non-`Send`).
        let mut rows = client
            .query_raw(
                "SELECT id FROM groups WHERE name = 'system-status'",
                hiqlite::params!(),
            )
            .await
            .map_err(|e| GroupError::Db(e.to_string()))?;
        let Some(row) = rows.first_mut() else {
            return Ok(user.is_admin);
        };
        Uuid::parse_str(&row.get::<String>("id"))
            .map_err(|e| GroupError::BadRequest(e.to_string()))?
    };
    let level = crate::services::groups::has_group_access(client, &group_id, user).await?;
    Ok(level.is_some())
}

/// Markdown section appended to capable agents' context prompts: a current
/// snapshot table plus a compact per-resource time series (newest first)
/// enough for a mermaid `timeline`/`stateDiagram`.
pub fn render_agent_section(
    report: &[SystemHealthEntryDb],
    history: &[SystemHealthEntryDb],
) -> String {
    let mut s = String::from("## System Health\n\n### Current Status\n\n");
    s.push_str(&render_status_table(report));
    s.push_str("\n### Recent History (time series, newest first)\n\n");
    s.push_str("| resource | status | created_at |\n");
    s.push_str("|----------|--------|------------|\n");
    for h in history.iter().take(10) {
        s.push_str(&format!(
            "| {} | {} | {} |\n",
            h.resource, h.status, h.created_at
        ));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn test_health_status_icon_mapping() {
        assert_eq!(HealthStatus::Ok.icon(), "✔");
        assert_eq!(HealthStatus::Warn.icon(), "▲");
        assert_eq!(HealthStatus::Missing.icon(), "✖");
        assert_eq!(HealthStatus::Error.icon(), "✘");
        assert_eq!(HealthStatus::Unknown.icon(), "–");
        assert_eq!(HealthStatus::Ok.as_str(), "ok");
        assert_eq!(HealthStatus::Missing.as_str(), "missing");
        assert_eq!(HealthStatus::from_str("ok").unwrap(), HealthStatus::Ok);
        assert!(HealthStatus::from_str("bogus").is_err());
    }

    #[test]
    fn test_parse_version() {
        assert_eq!(
            parse_version("git version 2.43.0", &["git version "]).as_deref(),
            Some("2.43.0")
        );
        assert_eq!(parse_version("npx/10.8.2", &[]).as_deref(), Some("10.8.2"));
        assert_eq!(
            parse_version("Docker version 27.3.1, build abc", &["Docker version "]).as_deref(),
            Some("27.3.1")
        );
        assert_eq!(parse_version("junk", &[]), None);
        assert_eq!(parse_version("", &[]), None);
        assert_eq!(parse_version("v2.0.0", &[]).as_deref(), Some("v2.0.0"));
    }

    #[test]
    fn test_find_in_path_with_synthetic_path() {
        let dir = tempfile::TempDir::new().unwrap();
        let script = dir.path().join("fakebin");
        std::fs::write(&script, "#!/bin/sh\necho 1.2.3\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let path_var = dir.path().to_string_lossy().to_string();
        let found = find_in_path_with_path("fakebin", &path_var);
        assert!(found.is_some(), "executable in PATH should be found");
        assert_eq!(found.unwrap(), script);

        // A non-executable file is skipped.
        let plain = dir.path().join("plainbin");
        std::fs::write(&plain, "data").unwrap();
        #[cfg(unix)]
        {
            std::fs::set_permissions(&plain, std::fs::Permissions::from_mode(0o644)).unwrap();
        }
        assert!(find_in_path_with_path("plainbin", &path_var).is_none());

        // Missing bin → None.
        assert!(find_in_path_with_path("does-not-exist", &path_var).is_none());
    }

    #[test]
    fn test_install_method_heuristics() {
        assert_eq!(install_method(Path::new("/home/u/.cargo/bin/git")), "cargo");
        assert_eq!(
            install_method(Path::new("/usr/local/lib/node_modules/.bin/npm")),
            "npm"
        );
        assert_eq!(
            install_method(Path::new("/home/u/.npm-global/bin/rtk")),
            "npm"
        );
        assert_eq!(install_method(Path::new("/usr/bin/git")), "system");
        assert_eq!(install_method(Path::new("/opt/custom/bin/tool")), "other");
    }

    #[test]
    fn test_playwright_browsers_honors_env() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("chromium-1148")).unwrap();
        std::fs::create_dir_all(dir.path().join("firefox-1400")).unwrap();
        std::fs::write(dir.path().join("not-a-dir"), "x").unwrap();
        // PLAYWRIGHT_BROWSERS_PATH is read from the env.
        std::env::set_var("PLAYWRIGHT_BROWSERS_PATH", dir.path());
        let browsers = playwright_browsers();
        assert!(browsers.iter().any(|b| b == "chromium-1148"));
        assert!(browsers.iter().any(|b| b == "firefox-1400"));
        assert!(!browsers.iter().any(|b| b == "not-a-dir"));
        std::env::remove_var("PLAYWRIGHT_BROWSERS_PATH");
    }

    #[test]
    fn test_bin_required_predicates() {
        let mut cfg = OfmConfig {
            rauthy_enabled: false,
            ..Default::default()
        };
        assert!(bin_required(&cfg, "bin:git"));
        assert!(bin_required(&cfg, "bin:opencode"));
        assert!(!bin_required(&cfg, "bin:docker"));
        assert!(!bin_required(&cfg, "bin:gh"));
        cfg.rauthy_enabled = true;
        assert!(bin_required(&cfg, "bin:docker"));
        assert!(!bin_required(&cfg, "bin:rtk"));
    }

    #[test]
    fn test_render_markdown_contains_sections_and_icons() {
        let rows = vec![
            SystemHealthEntryDb {
                id: 1,
                category: "dependency".into(),
                resource: "bin:git".into(),
                status: "ok".into(),
                detail: "git found at /usr/bin/git".into(),
                metadata: serde_json::json!({"version": "2.43.0", "path": "/usr/bin/git"})
                    .to_string(),
                created_at: "2026-01-01 00:00:00".into(),
            },
            SystemHealthEntryDb {
                id: 2,
                category: "live".into(),
                resource: "live:opencode-pool".into(),
                status: "ok".into(),
                detail: "1 pooled opencode server(s)".into(),
                metadata: serde_json::json!({}).to_string(),
                created_at: "2026-01-01 00:00:00".into(),
            },
        ];
        let md = render_markdown(&rows);
        assert!(md.contains("## Dependency Check"));
        assert!(md.contains("## Live System Health"));
        assert!(md.contains("✔"), "ok entries should carry the ok icon");
        assert!(md.contains("**bin:git**"));
        assert!(md.contains("| status | resource |"));
        assert!(md.contains("2.43.0"));
    }

    #[test]
    fn test_render_json_shape_and_running_services() {
        let rows = vec![
            SystemHealthEntryDb {
                id: 1,
                category: "live".into(),
                resource: "live:opencode-pool".into(),
                status: "ok".into(),
                detail: "1 pooled opencode server(s)".into(),
                metadata: serde_json::json!({"pid": 1234}).to_string(),
                created_at: "2026-01-01 00:00:00".into(),
            },
            SystemHealthEntryDb {
                id: 2,
                category: "live".into(),
                resource: "live:gh".into(),
                status: "error".into(),
                detail: "gh api user failed".into(),
                metadata: serde_json::json!({}).to_string(),
                created_at: "2026-01-01 00:00:00".into(),
            },
            SystemHealthEntryDb {
                id: 3,
                category: "dependency".into(),
                resource: "bin:git".into(),
                status: "ok".into(),
                detail: "git found".into(),
                metadata: serde_json::json!({}).to_string(),
                created_at: "2026-01-01 00:00:00".into(),
            },
        ];
        let json = render_json(&rows);
        assert!(json["generated_at"].is_string());
        assert_eq!(json["running_services"], 1, "only live+ok counts");
        assert_eq!(json["entries"].as_array().unwrap().len(), 3);
        let first = &json["entries"][0];
        assert_eq!(first["category"], "live");
        assert_eq!(first["resource"], "live:opencode-pool");
        assert_eq!(first["metadata"]["pid"], 1234);
    }

    #[test]
    fn test_render_markdown_deps_only() {
        let entries = vec![
            HealthEntry {
                category: "dependency",
                resource: "bin:git".into(),
                status: HealthStatus::Ok,
                detail: "git found at /usr/bin/git".into(),
                metadata: json!({}),
            },
            HealthEntry {
                category: "dependency",
                resource: "bin:rtk".into(),
                status: HealthStatus::Missing,
                detail: "rtk not found in PATH".into(),
                metadata: json!({}),
            },
        ];
        let md = render_markdown_deps_only(&entries);
        assert!(md.contains("✔ **bin:git** — ok"));
        assert!(md.contains("✖ **bin:rtk** — missing"));
    }

    #[test]
    fn test_render_agent_section() {
        let report = vec![SystemHealthEntryDb {
            id: 1,
            category: "live".into(),
            resource: "live:hiqlite".into(),
            status: "ok".into(),
            detail: "hiqlite cluster healthy=true".into(),
            metadata: serde_json::json!({}).to_string(),
            created_at: "2026-01-01 00:00:00".into(),
        }];
        let history = vec![
            SystemHealthEntryDb {
                id: 10,
                category: "live".into(),
                resource: "live:hiqlite".into(),
                status: "ok".into(),
                detail: "".into(),
                metadata: serde_json::json!({}).to_string(),
                created_at: "2026-01-01 00:00:00".into(),
            },
            SystemHealthEntryDb {
                id: 9,
                category: "live".into(),
                resource: "live:hiqlite".into(),
                status: "error".into(),
                detail: "".into(),
                metadata: serde_json::json!({}).to_string(),
                created_at: "2025-12-31 00:00:00".into(),
            },
        ];
        let md = render_agent_section(&report, &history);
        assert!(md.contains("## System Health"));
        assert!(md.contains("### Current Status"));
        assert!(md.contains("### Recent History (time series, newest first)"));
        assert!(md.contains("| resource | status | created_at |"));
        assert!(md.contains("| live:hiqlite | ok | 2026-01-01 00:00:00 |"));
    }

    async fn make_client() -> (Client, tempfile::TempDir) {
        let tmp = tempfile::TempDir::new().unwrap();
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
        (client, tmp)
    }

    fn sample_entry(resource: &str, status: HealthStatus, category: &'static str) -> HealthEntry {
        HealthEntry {
            category,
            resource: resource.to_string(),
            status,
            detail: format!("{resource} detail"),
            metadata: json!({"version": "1.0"}),
        }
    }

    #[tokio::test]
    async fn test_refresh_and_prune_and_history() {
        let (client, _tmp) = make_client().await;
        // Write more rows than the retention cap (one entry per refresh), then
        // a final live row — the table must be pruned back to the cap.
        for _ in 0..(MAX_ROWS_PER_PRUNE + 20) {
            let entries = vec![sample_entry("bin:git", HealthStatus::Ok, "dependency")];
            refresh_entries(&client, &entries).await.unwrap();
        }
        let live_entries = vec![sample_entry("live:hiqlite", HealthStatus::Ok, "live")];
        refresh_entries(&client, &live_entries).await.unwrap();

        let mut rows = client
            .query_raw(
                "SELECT COUNT(*) AS cnt FROM system_health_entry",
                hiqlite::params!(),
            )
            .await
            .unwrap();
        let total: i64 = rows.first_mut().unwrap().get("cnt");
        assert_eq!(total, MAX_ROWS_PER_PRUNE, "pruned to the retention cap");

        let latest = latest_report(&client).await.unwrap();
        assert_eq!(latest.len(), 2);
        assert!(latest
            .iter()
            .all(|e| e.resource == "bin:git" || e.resource == "live:hiqlite"));

        // History ordering: newest first, limited.
        let history = history_report(&client, Some("bin:git"), 5).await.unwrap();
        assert!(history.len() <= 5);
        assert!(history.windows(2).all(|w| w[0].id > w[1].id));
        assert!(history.iter().all(|e| e.resource == "bin:git"));
    }

    #[tokio::test]
    async fn test_user_can_use_system_health_capability() {
        let (client, _tmp) = make_client().await;
        let admin_id = uuid::Uuid::new_v4();
        let scoped_id = uuid::Uuid::new_v4();
        let plain_id = uuid::Uuid::new_v4();
        let now = utc_now();

        // The group seed requires an admin row (mirrors `main`:
        // `ensure_default_user` + `ensure_admins_group` run first). Insert the
        // admin before seeding the group.
        client
            .execute(
                "INSERT INTO users (id, username, is_admin, is_active, created_at) VALUES ($1, $2, 1, 1, $3)",
                hiqlite::params!(admin_id.to_string(), "admin-u", &now),
            )
            .await
            .unwrap();
        crate::db::ensure_system_status_group(&client)
            .await
            .unwrap();

        for (id, username) in [(scoped_id, "scoped-u"), (plain_id, "plain-u")] {
            client
                .execute(
                    "INSERT INTO users (id, username, is_admin, is_active, created_at) VALUES ($1, $2, 0, 1, $3)",
                    hiqlite::params!(id.to_string(), username, &now),
                )
                .await
                .unwrap();
        }
        client
            .execute(
                "UPDATE users SET scopes = 'openid system-status' WHERE id = $1",
                hiqlite::params!(scoped_id.to_string()),
            )
            .await
            .unwrap();

        let auth = |id: uuid::Uuid, is_admin: bool| AuthUser {
            user_id: id,
            username: "u".into(),
            oidc_subject: None,
            is_admin,
            is_technical: false,
        };
        assert!(user_can_use_system_health(&client, &auth(admin_id, true))
            .await
            .unwrap());
        assert!(user_can_use_system_health(&client, &auth(scoped_id, false))
            .await
            .unwrap());
        assert!(!user_can_use_system_health(&client, &auth(plain_id, false))
            .await
            .unwrap());
    }
}
