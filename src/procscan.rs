//! Process introspection for `ofm health` and the startup restart guard.
//!
//! All reading is best-effort: unreadable/ephemeral `/proc` entries are
//! skipped. Teardown is strictly precise per AGENTS.md:
//!
//! - **exact-PID kill** for the ofm root and shells/rauthy spawners
//!   (`SIGTERM` → grace → `SIGKILL`);
//! - **process-group kill only for groups OFM created** — `opencode serve`
//!   runs `process_group(0)` so its group id == child pid (`kill(-pid, ...)`);
//! - **named-container removal only** for `ofm-rauthy-<fnv64>` containers
//!   (`docker rm -f`), never other containers;
//! - rauthy's `docker run` spawner is not in its own process group — kill by
//!   exact PID only.
//!
//! Attribution key is `OFM_FOOTPRINT` in `/proc/{pid}/environ` (children
//! inherit it). When the ofm root is SIGKILLed its environ is gone, so a
//! footprint pid-file (`{footprint}/ofm.pid`) bridges dead-instance
//! attribution: written at startup, removed on clean shutdown, left behind on
//! crash/SIGKILL.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

const DEFAULT_GRACE: Duration = Duration::from_secs(3);

/// Best-effort snapshot of one `/proc` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcInfo {
    pub pid: i32,
    pub ppid: i32,
    pub pgid: i32,
    pub comm: String,
    pub cmdline: Vec<String>,
    pub environ: HashMap<String, String>,
    pub rss_kb: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassKind {
    Ofm,
    OpencodeServe,
    RauthyDockerRun,
    Shell,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classified {
    pub info: ProcInfo,
    pub kind: ClassKind,
    pub footprint: Option<String>,
    pub container: Option<String>,
}

/// Scan all readable `/proc/[0-9]+` entries.
pub fn scan() -> Vec<ProcInfo> {
    #[cfg(unix)]
    {
        let mut out = Vec::new();
        let Ok(entries) = std::fs::read_dir("/proc") else {
            return out;
        };
        for entry in entries.flatten() {
            let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|s| s.parse::<i32>().ok())
            else {
                continue;
            };
            if let Some(info) = read_proc(pid) {
                out.push(info);
            }
        }
        out
    }
    #[cfg(not(unix))]
    {
        Vec::new()
    }
}

/// Convenience: `scan()` + `classify()` for callers that never need the raw
/// rows.
pub fn scan_classified() -> Vec<Classified> {
    scan().iter().map(classify).collect()
}

#[cfg(unix)]
fn read_proc(pid: i32) -> Option<ProcInfo> {
    let base = Path::new("/proc").join(pid.to_string());
    let stat = std::fs::read_to_string(base.join("stat")).ok()?;
    // stat: pid (comm) state ppid pgrp ...  — comm may contain spaces/parens.
    let open = stat.find('(')?;
    let close = stat.rfind(')')?;
    let comm = stat[open + 1..close].to_string();
    let rest: Vec<&str> = stat[close + 2..].split_whitespace().collect();
    let ppid = rest.get(1).and_then(|s| s.parse::<i32>().ok()).unwrap_or(0);
    let pgid = rest.get(2).and_then(|s| s.parse::<i32>().ok()).unwrap_or(0);

    let cmdline = read_nul(&base.join("cmdline"));
    let environ = read_environ(&base.join("environ"));
    let rss_kb = read_statm_rss(&base.join("statm"));

    Some(ProcInfo {
        pid,
        ppid,
        pgid,
        comm,
        cmdline,
        environ,
        rss_kb,
    })
}

/// Parse a NUL-separated `cmdline`/`environ`-style file.
fn read_nul(path: &Path) -> Vec<String> {
    std::fs::read(path)
        .map(|bytes| {
            bytes
                .split(|&b| b == 0)
                .filter(|s| !s.is_empty())
                .map(|s| String::from_utf8_lossy(s).into_owned())
                .collect()
        })
        .unwrap_or_default()
}

fn read_environ(path: &Path) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Ok(bytes) = std::fs::read(path) {
        for kv in bytes.split(|&b| b == 0).filter(|s| !s.is_empty()) {
            if let Some(eq) = kv.iter().position(|&b| b == b'=') {
                let key = String::from_utf8_lossy(&kv[..eq]).into_owned();
                let val = String::from_utf8_lossy(&kv[eq + 1..]).into_owned();
                map.insert(key, val);
            }
        }
    }
    map
}

/// Resident set size in KB from `/proc/{pid}/statm` (4096-byte pages).
pub fn rss_kb(pid: u32) -> Option<u64> {
    #[cfg(unix)]
    {
        let content = std::fs::read_to_string(format!("/proc/{pid}/statm")).ok()?;
        let mut fields = content.split_whitespace();
        let _size = fields.next()?;
        let resident_pages = fields.next()?.parse::<u64>().ok()?;
        Some(resident_pages * 4)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        None
    }
}

fn read_statm_rss(path: &Path) -> Option<u64> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut fields = content.split_whitespace();
    let _size = fields.next()?;
    let resident_pages = fields.next()?.parse::<u64>().ok()?;
    Some(resident_pages * 4)
}

fn is_shell_comm(comm: &str) -> bool {
    matches!(comm, "bash" | "sh" | "zsh" | "fish")
}

fn is_rauthy_container_token(arg: &str) -> Option<String> {
    let name = arg.strip_prefix("ofm-rauthy-")?;
    if name.len() == 16 && name.bytes().all(|b| b.is_ascii_hexdigit()) {
        Some(format!("ofm-rauthy-{name}"))
    } else {
        None
    }
}

/// Classify a single process: ofm root, `opencode serve`, a rauthy
/// `docker run` spawner, an ofm-attributed shell, or other.
pub fn classify(p: &ProcInfo) -> Classified {
    let footprint = p.environ.get("OFM_FOOTPRINT").cloned();
    let mut kind = ClassKind::Other;
    let mut container = None;

    if p.comm == "ofm" {
        kind = ClassKind::Ofm;
    } else if p.cmdline.iter().any(|a| a == "opencode") && p.cmdline.iter().any(|a| a == "serve") {
        kind = ClassKind::OpencodeServe;
    } else if p.cmdline.iter().any(|a| a == "docker") && p.cmdline.iter().any(|a| a == "run") {
        if let Some(name) = p.cmdline.iter().find_map(|a| is_rauthy_container_token(a)) {
            kind = ClassKind::RauthyDockerRun;
            container = Some(name);
        }
    } else if is_shell_comm(&p.comm) && footprint.is_some() {
        kind = ClassKind::Shell;
    }

    Classified {
        info: p.clone(),
        kind,
        footprint,
        container,
    }
}

// ── Footprint attribution ────────────────────────────────────────────────────

/// Processes attributed to `fp`: by `OFM_FOOTPRINT` environ match, or (for
/// shells) by ppid ancestry to a live ofm of `fp`.
pub fn collect_for_footprint(scan: &[Classified], fp: &str) -> Vec<Classified> {
    let live_ofm: std::collections::HashSet<i32> = scan
        .iter()
        .filter(|c| c.kind == ClassKind::Ofm && c.footprint.as_deref() == Some(fp))
        .map(|c| c.info.pid)
        .collect();
    let ppids: HashMap<i32, i32> = scan.iter().map(|c| (c.info.pid, c.info.ppid)).collect();

    let mut out = Vec::new();
    for c in scan {
        if c.footprint.as_deref() == Some(fp) {
            out.push(c.clone());
        } else if is_shell_comm(&c.info.comm) && has_ancestor_in(c.info.pid, &ppids, &live_ofm) {
            // A shell launched by ofm inherits the ancestry even when its own
            // environ lacks the footprint marker (e.g. re-exec'd).
            let mut cls = c.clone();
            if cls.footprint.is_none() {
                cls.footprint = Some(fp.to_string());
            }
            out.push(cls);
        }
    }
    out
}

fn has_ancestor_in(
    pid: i32,
    ppids: &HashMap<i32, i32>,
    targets: &std::collections::HashSet<i32>,
) -> bool {
    let mut cur = ppids.get(&pid).copied();
    let mut guard = 0;
    while let Some(p) = cur {
        if targets.contains(&p) {
            return true;
        }
        if guard > 64 {
            return false;
        }
        guard += 1;
        cur = ppids.get(&p).copied();
    }
    false
}

/// Aggregate every possibly-ofm-descended resource class across the machine.
#[derive(Debug, Default)]
pub struct GlobalReport {
    pub ofm_instances: Vec<Classified>,
    pub opencode: Vec<Classified>,
    pub rauthy_spawners: Vec<Classified>,
    pub containers: Vec<String>,
    pub shells: Vec<Classified>,
    pub total_rss_kb: u64,
}

/// Build a `GlobalReport` from a classified scan plus the caller-supplied
/// `ofm-rauthy-*` container list (from `list_rauthy_containers`).
pub fn collect_global(scan: &[Classified], containers: &[String]) -> GlobalReport {
    let mut report = GlobalReport::default();
    for c in scan {
        match c.kind {
            ClassKind::Ofm => report.ofm_instances.push(c.clone()),
            ClassKind::OpencodeServe => report.opencode.push(c.clone()),
            ClassKind::RauthyDockerRun => report.rauthy_spawners.push(c.clone()),
            ClassKind::Shell => report.shells.push(c.clone()),
            ClassKind::Other => {}
        }
        report.total_rss_kb = report
            .total_rss_kb
            .saturating_add(c.info.rss_kb.unwrap_or(0));
    }
    report.containers = containers.to_vec();
    report
}

/// List running `ofm-rauthy-*` containers by name (`docker ps --filter`).
pub fn list_rauthy_containers() -> Vec<String> {
    let out = std::process::Command::new("docker")
        .args([
            "ps",
            "--filter",
            "name=ofm-rauthy",
            "--format",
            "{{.Names}}",
        ])
        .output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| s.starts_with("ofm-rauthy-"))
            .collect(),
        _ => Vec::new(),
    }
}

// ── Pid-file markers ─────────────────────────────────────────────────────────

pub fn pid_file_path(footprint: &str) -> PathBuf {
    Path::new(footprint).join("ofm.pid")
}

pub fn read_pid_file(footprint: &str) -> Option<i32> {
    std::fs::read_to_string(pid_file_path(footprint))
        .ok()?
        .trim()
        .parse()
        .ok()
}

pub fn write_pid_file(footprint: &str, pid: i32) -> std::io::Result<()> {
    let path = pid_file_path(footprint);
    std::fs::write(&path, pid.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

pub fn remove_pid_file(footprint: &str) {
    let _ = std::fs::remove_file(pid_file_path(footprint));
}

/// Bounded (depth ≤ 3) walk for `ofm.pid` under `home` and `cwd`, returning
/// `(path, pid)` pairs. Used by `ofm health --teardown <PID>` to attribute a
/// dead PID to a footprint.
pub fn find_pid_files(home: &Path, cwd: &Path) -> Vec<(PathBuf, i32)> {
    let mut out = Vec::new();
    let walk = |root: &Path, out: &mut Vec<(PathBuf, i32)>| {
        let mut stack: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];
        while let Some((dir, depth)) = stack.pop() {
            if depth > 3 {
                continue;
            }
            let Ok(rd) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in rd.flatten() {
                let p = entry.path();
                if p.file_name().map(|n| n == "ofm.pid").unwrap_or(false) {
                    if let Ok(content) = std::fs::read_to_string(&p) {
                        if let Ok(pid) = content.trim().parse::<i32>() {
                            out.push((p, pid));
                        }
                    }
                } else if p.is_dir() {
                    stack.push((p, depth + 1));
                }
            }
        }
    };
    if home.is_dir() {
        walk(home, &mut out);
    }
    if cwd != home && cwd.is_dir() {
        walk(cwd, &mut out);
    }
    out
}

/// What `ofm health --teardown <PID>` (and friends) target.
#[derive(Debug, Clone)]
pub struct InstanceTarget {
    pub footprint: Option<String>,
    pub live: Option<ProcInfo>,
}

/// Resolve `pid` to an instance: a live process (attributed via environ) or a
/// dead PID matched against a footprint pid-file.
pub fn resolve_instance(
    scan: &[ProcInfo],
    pid: i32,
    pid_files: &[(PathBuf, i32)],
) -> Option<InstanceTarget> {
    if let Some(live) = scan.iter().find(|p| p.pid == pid) {
        let footprint = live.environ.get("OFM_FOOTPRINT").cloned();
        return Some(InstanceTarget {
            footprint,
            live: Some(live.clone()),
        });
    }
    for (path, file_pid) in pid_files {
        if *file_pid == pid {
            let footprint = path
                .parent()
                .and_then(|p| p.to_str())
                .map(|s| s.to_string());
            return Some(InstanceTarget {
                footprint,
                live: None,
            });
        }
    }
    None
}

// ── Precise teardown primitives ──────────────────────────────────────────────

pub fn signal_pid(pid: i32, sig: i32) {
    #[cfg(unix)]
    unsafe {
        libc::kill(pid, sig);
    }
    #[cfg(not(unix))]
    {
        let _ = (pid, sig);
    }
}

/// `/proc/{pid}/stat` state char (after the `(comm)` field).
fn proc_state(pid: i32) -> Option<char> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let close = stat.rfind(')')?;
    stat[close + 1..].split_whitespace().next()?.chars().next()
}

pub fn pid_alive(pid: i32) -> bool {
    #[cfg(unix)]
    {
        if unsafe { libc::kill(pid, 0) != 0 } {
            return false;
        }
        // A zombie holds no resources; treat it as gone so teardown never
        // hangs waiting for a child its parent has yet to reap.
        proc_state(pid).map(|s| s != 'Z').unwrap_or(true)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

/// `SIGTERM` → wait up to `grace` → `SIGKILL`. Returns `true` once the PID is
/// gone.
pub fn kill_pid_tree(pid: i32, grace: Duration) -> bool {
    signal_pid(pid, libc::SIGTERM);
    let deadline = std::time::Instant::now() + grace;
    while std::time::Instant::now() < deadline {
        if !pid_alive(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    if pid_alive(pid) {
        signal_pid(pid, libc::SIGKILL);
    }
    wait_until_dead(pid)
}

fn wait_until_dead(pid: i32) -> bool {
    for _ in 0..20 {
        if !pid_alive(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    !pid_alive(pid)
}

/// Kill an entire process group. Callers MUST only pass `opencode serve` PIDs
/// — OFM created those groups (`process_group(0)`), so `pgid == pid`.
pub fn kill_owned_group(pgid: i32, grace: Duration) -> bool {
    #[cfg(unix)]
    unsafe {
        libc::kill(-pgid, libc::SIGTERM);
    }
    let deadline = std::time::Instant::now() + grace;
    let mut alive = group_alive(pgid);
    while std::time::Instant::now() < deadline {
        alive = group_alive(pgid);
        if !alive {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    if alive {
        #[cfg(unix)]
        unsafe {
            libc::kill(-pgid, libc::SIGKILL);
        }
    }
    for _ in 0..20 {
        if !group_alive(pgid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    !group_alive(pgid)
}

fn group_alive(pgid: i32) -> bool {
    #[cfg(unix)]
    unsafe {
        libc::kill(-pgid, 0) == 0
    }
    #[cfg(not(unix))]
    {
        let _ = pgid;
        false
    }
}

/// `docker rm -f` a named container. Only ever called with `ofm-rauthy-*`
/// names derived from `classify`/`list_rauthy_containers`.
pub fn docker_rm(container: &str) -> bool {
    std::process::Command::new("docker")
        .args(["rm", "-f", container])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Outcome of a footprint-scoped teardown.
#[derive(Debug, Default)]
pub struct TeardownResult {
    pub ofm_killed: usize,
    pub opencode_groups: usize,
    pub shells_killed: usize,
    pub rauthy_spawners_killed: usize,
    pub containers_removed: usize,
    pub survivors: Vec<Classified>,
}

/// Precisely tear down every ofm-descended resource for `target`'s footprint:
/// live ofm root (exact PID), `opencode serve` (owned groups), shells and the
/// rauthy `docker run` spawner (exact PID), `ofm-rauthy-*` containers (by
/// name). Removes the footprint pid-file, then re-scans to verify.
pub fn teardown_footprint(target: &InstanceTarget, scan: &[Classified]) -> TeardownResult {
    let mut result = TeardownResult::default();
    let fp = target.footprint.clone();
    let stragglers: Vec<Classified> = if let Some(fp) = &fp {
        collect_for_footprint(scan, fp)
    } else {
        scan.to_vec()
    };

    // Everything collected here is footprint-attributed (it carries
    // `OFM_FOOTPRINT` or is a shell in the ofm ancestry), so every kind is
    // killed — including `Other` processes (e.g. a `sleep` that `sh -c`
    // `exec`'d into). Each kill is an exact PID (or an owned group for
    // `opencode serve`), so teardown stays precise.
    for c in &stragglers {
        match c.kind {
            ClassKind::Ofm => {
                if kill_pid_tree(c.info.pid, DEFAULT_GRACE) {
                    result.ofm_killed += 1;
                }
            }
            ClassKind::OpencodeServe => {
                if kill_owned_group(c.info.pid, DEFAULT_GRACE) {
                    result.opencode_groups += 1;
                }
            }
            ClassKind::Shell => {
                if kill_pid_tree(c.info.pid, DEFAULT_GRACE) {
                    result.shells_killed += 1;
                }
            }
            ClassKind::RauthyDockerRun => {
                if kill_pid_tree(c.info.pid, DEFAULT_GRACE) {
                    result.rauthy_spawners_killed += 1;
                }
            }
            ClassKind::Other => {
                let _ = kill_pid_tree(c.info.pid, DEFAULT_GRACE);
            }
        }
        if let Some(container) = &c.container {
            if docker_rm(container) {
                result.containers_removed += 1;
            }
        }
    }

    if let Some(fp) = &fp {
        remove_pid_file(fp);
    }

    let fresh = scan_classified();
    result.survivors = if let Some(fp) = &fp {
        collect_for_footprint(&fresh, fp)
    } else {
        fresh
    };
    result
}

/// Outcome of a machine-wide teardown.
#[derive(Debug, Default)]
pub struct GlobalTeardownResult {
    pub ofm_killed: usize,
    pub opencode_groups: usize,
    pub rauthy_spawners_killed: usize,
    pub shells_killed: usize,
    pub containers_removed: usize,
    pub survivors: Vec<Classified>,
    pub containers_remaining: Vec<String>,
}

/// Precisely eradicate all possibly-ofm-descended resource usage across every
/// reported instance: opencode servers (owned groups), ofm roots + rauthy
/// spawners + shells (exact PID), and `ofm-rauthy-*` containers (by name).
pub fn teardown_global(report: &GlobalReport) -> GlobalTeardownResult {
    for c in &report.ofm_instances {
        let _ = kill_pid_tree(c.info.pid, DEFAULT_GRACE);
    }
    for c in &report.opencode {
        let _ = kill_owned_group(c.info.pid, DEFAULT_GRACE);
    }
    for c in &report.rauthy_spawners {
        let _ = kill_pid_tree(c.info.pid, DEFAULT_GRACE);
    }
    for c in &report.shells {
        let _ = kill_pid_tree(c.info.pid, DEFAULT_GRACE);
    }
    let mut containers_removed = 0;
    for name in &report.containers {
        if docker_rm(name) {
            containers_removed += 1;
        }
    }

    let fresh = scan_classified();
    let survivors: Vec<Classified> = fresh
        .into_iter()
        .filter(|c| {
            matches!(
                c.kind,
                ClassKind::Ofm
                    | ClassKind::OpencodeServe
                    | ClassKind::RauthyDockerRun
                    | ClassKind::Shell
            )
        })
        .collect();
    GlobalTeardownResult {
        ofm_killed: report.ofm_instances.len(),
        opencode_groups: report.opencode.len(),
        rauthy_spawners_killed: report.rauthy_spawners.len(),
        shells_killed: report.shells.len(),
        containers_removed,
        survivors,
        containers_remaining: list_rauthy_containers(),
    }
}

// ── Restart guard ────────────────────────────────────────────────────────────

#[derive(Debug, PartialEq, Eq)]
pub enum RestartStatus {
    Clean,
    Dirty(Vec<Classified>),
    Blocked(i32),
}

/// Startup guard: `Blocked(live_pid)` when a live ofm owns `footprint` (other
/// than `current_pid`) or the pid-file pid is still alive; `Dirty(stragglers)`
/// when no live ofm exists but leftover ofm-descended resources do; `Clean`
/// otherwise.
pub fn restart_guard(
    scan: &[Classified],
    footprint: &str,
    current_pid: u32,
    pid_file_pid: Option<i32>,
) -> RestartStatus {
    for c in scan {
        if c.kind == ClassKind::Ofm
            && c.footprint.as_deref() == Some(footprint)
            && c.info.pid != current_pid as i32
        {
            return RestartStatus::Blocked(c.info.pid);
        }
    }
    if let Some(fp_pid) = pid_file_pid {
        if fp_pid != current_pid as i32 && pid_alive(fp_pid) {
            return RestartStatus::Blocked(fp_pid);
        }
    }
    let leftover: Vec<Classified> = collect_for_footprint(scan, footprint)
        .into_iter()
        .filter(|c| {
            matches!(
                c.kind,
                ClassKind::OpencodeServe | ClassKind::Shell | ClassKind::RauthyDockerRun
            )
        })
        .collect();
    if leftover.is_empty() {
        RestartStatus::Clean
    } else {
        RestartStatus::Dirty(leftover)
    }
}

/// Async wrapper used by `main`'s restart guard cleanup: precisely kills
/// leftover opencode servers (owned groups), shells and rauthy spawners (exact
/// PIDs). The rauthy container is reaped by `start_rauthy`'s own `docker rm -f`.
pub async fn cleanup_stragglers(stragglers: &[Classified]) {
    for c in stragglers {
        match c.kind {
            ClassKind::OpencodeServe => {
                let _ = kill_owned_group(c.info.pid, DEFAULT_GRACE);
            }
            ClassKind::Shell | ClassKind::RauthyDockerRun => {
                let _ = kill_pid_tree(c.info.pid, DEFAULT_GRACE);
            }
            _ => {}
        }
    }
}

/// Markdown-ish render of classified processes for reports.
pub fn render(classified: &[Classified]) -> String {
    let mut s = String::new();
    for c in classified {
        s.push_str(&format!(
            "- pid {} ({:?}) — {:?}\n",
            c.info.pid, c.info.comm, c.kind
        ));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc_info(
        pid: i32,
        ppid: i32,
        comm: &str,
        cmdline: &[&str],
        footprint: Option<&str>,
    ) -> ProcInfo {
        let mut environ = HashMap::new();
        if let Some(fp) = footprint {
            environ.insert("OFM_FOOTPRINT".into(), fp.to_string());
        }
        ProcInfo {
            pid,
            ppid,
            pgid: pid,
            comm: comm.to_string(),
            cmdline: cmdline.iter().map(|s| s.to_string()).collect(),
            environ,
            rss_kb: Some(1024),
        }
    }

    #[test]
    fn test_read_nul_separated() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("cmdline");
        let mut cmd_bytes = b"opencode\0serve\0--port\0".to_vec();
        cmd_bytes.extend_from_slice(b"1234\0");
        std::fs::write(&path, &cmd_bytes).unwrap();
        assert_eq!(read_nul(&path), vec!["opencode", "serve", "--port", "1234"]);

        let env_path = dir.path().join("environ");
        std::fs::write(&env_path, b"OFM_FOOTPRINT=/tmp/fp\0PATH=/usr/bin\0").unwrap();
        let env = read_environ(&env_path);
        assert_eq!(
            env.get("OFM_FOOTPRINT").map(String::as_str),
            Some("/tmp/fp")
        );
        assert_eq!(env.get("PATH").map(String::as_str), Some("/usr/bin"));
    }

    #[test]
    fn test_classify_ofm_root() {
        let info = proc_info(100, 1, "ofm", &["ofm"], Some("/tmp/fp"));
        let c = classify(&info);
        assert_eq!(c.kind, ClassKind::Ofm);
        assert_eq!(c.footprint.as_deref(), Some("/tmp/fp"));
    }

    #[test]
    fn test_classify_opencode_serve() {
        let info = proc_info(
            200,
            100,
            "opencode",
            &["opencode", "serve", "--port", "9999"],
            Some("/tmp/fp"),
        );
        let c = classify(&info);
        assert_eq!(c.kind, ClassKind::OpencodeServe);
        assert_eq!(c.footprint.as_deref(), Some("/tmp/fp"));
    }

    #[test]
    fn test_classify_rauthy_docker_spawner() {
        let name = crate::rauthy::container_name("/tmp/fp");
        let info = proc_info(
            300,
            100,
            "docker",
            &[
                "docker",
                "run",
                "--name",
                &name,
                "ghcr.io/sebadob/rauthy:latest",
            ],
            Some("/tmp/fp"),
        );
        let c = classify(&info);
        assert_eq!(c.kind, ClassKind::RauthyDockerRun);
        assert_eq!(c.container.as_deref(), Some(name.as_str()));

        // A non-ofm docker run must not be classified.
        let other = proc_info(
            301,
            100,
            "docker",
            &["docker", "run", "--name", "postgres", "postgres"],
            Some("/tmp/fp"),
        );
        assert_eq!(classify(&other).kind, ClassKind::Other);
    }

    #[test]
    fn test_classify_shell_with_footprint() {
        let info = proc_info(400, 100, "bash", &["bash"], Some("/tmp/fp"));
        assert_eq!(classify(&info).kind, ClassKind::Shell);
        // Shell without footprint but ofm ancestor gets attributed by ancestry.
        let orphan = proc_info(401, 100, "zsh", &["zsh"], None);
        assert_eq!(classify(&orphan).kind, ClassKind::Other);
    }

    #[test]
    fn test_collect_for_footprint_attribution() {
        let ofm = classify(&proc_info(100, 1, "ofm", &["ofm"], Some("/tmp/fp")));
        let serve = classify(&proc_info(
            200,
            100,
            "opencode",
            &["opencode", "serve", "--port", "1"],
            Some("/tmp/fp"),
        ));
        let shell_child = classify(&proc_info(201, 200, "bash", &["bash"], None));
        let other_fp = classify(&proc_info(
            500,
            1,
            "opencode",
            &["opencode", "serve", "--port", "2"],
            Some("/tmp/other"),
        ));
        let scan = vec![ofm, serve, shell_child, other_fp];
        let got = collect_for_footprint(&scan, "/tmp/fp");
        let pids: std::collections::HashSet<i32> = got.iter().map(|c| c.info.pid).collect();
        assert!(pids.contains(&100), "ofm root");
        assert!(pids.contains(&200), "opencode serve");
        assert!(
            pids.contains(&201),
            "shell without env inherits ancestry to ofm"
        );
        assert!(!pids.contains(&500), "other footprint excluded");
    }

    #[test]
    fn test_resolve_instance_live_and_dead() {
        let live = proc_info(100, 1, "ofm", &["ofm"], Some("/tmp/fp"));
        let pid_files = vec![(PathBuf::from("/tmp/fp/ofm.pid"), 900)];
        let scan = vec![live];

        let t = resolve_instance(&scan, 100, &pid_files).unwrap();
        assert!(t.live.is_some());
        assert_eq!(t.footprint.as_deref(), Some("/tmp/fp"));

        let dead = resolve_instance(&scan, 900, &pid_files).unwrap();
        assert!(dead.live.is_none());
        assert_eq!(dead.footprint.as_deref(), Some("/tmp/fp"));

        assert!(resolve_instance(&scan, 404, &pid_files).is_none());
    }

    #[test]
    fn test_restart_guard_states() {
        let current = std::process::id() as i32;
        // Clean: no ofm, no pid file, no stragglers.
        let scan: Vec<Classified> = vec![];
        assert_eq!(
            restart_guard(&scan, "/tmp/fp", current as u32, None),
            RestartStatus::Clean
        );

        // Blocked: a different live ofm owns the footprint.
        let other = classify(&proc_info(777, 1, "ofm", &["ofm"], Some("/tmp/fp")));
        assert_eq!(
            restart_guard(&[other], "/tmp/fp", current as u32, None),
            RestartStatus::Blocked(777)
        );

        // Dirty: no live ofm, but an opencode serve straggler remains.
        let straggler = classify(&proc_info(
            200,
            999,
            "opencode",
            &["opencode", "serve", "--port", "1"],
            Some("/tmp/fp"),
        ));
        match restart_guard(&[straggler], "/tmp/fp", current as u32, None) {
            RestartStatus::Dirty(list) => {
                assert_eq!(list.len(), 1);
                assert_eq!(list[0].info.pid, 200);
            }
            other => panic!("expected Dirty, got {other:?}"),
        }
    }

    #[test]
    fn test_find_pid_files_bounded_walk() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("ofm.pid"), "1234\n").unwrap();
        let nested = dir.path().join("a").join("b").join("c").join("d");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("ofm.pid"), "5678\n").unwrap();

        let found = find_pid_files(dir.path(), dir.path());
        let pids: Vec<i32> = found.iter().map(|(_, pid)| *pid).collect();
        assert!(pids.contains(&1234));
        // Depth 3 from the root reaches a/b/c/ofm.pid; the d/ofm.pid is at
        // depth 4 and must be skipped.
        assert!(!pids.contains(&5678), "depth 4 entry must be skipped");
    }

    #[test]
    fn test_pid_file_roundtrip() {
        let dir = tempfile::TempDir::new().unwrap();
        let fp = dir.path().to_str().unwrap();
        write_pid_file(fp, 4242).unwrap();
        assert_eq!(read_pid_file(fp), Some(4242));
        remove_pid_file(fp);
        assert_eq!(read_pid_file(fp), None);
    }
}
