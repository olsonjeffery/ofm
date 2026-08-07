//! End-to-end tests of the `ofm health` CLI via `env!("CARGO_BIN_EXE_ofm")`.
//!
//! These spawn the real binary as a subprocess; every scenario is designed to
//! be safe on a shared host (no bulk kills, only exact-PID teardown of
//! test-owned processes).

use std::path::Path;
use std::process::Command;

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_ofm")
}

fn run_health(args: &[&str]) -> std::process::Output {
    Command::new(binary())
        .arg("health")
        .args(args)
        .output()
        .expect("ofm health failed to spawn")
}

fn run_health_with_env(args: &[&str], envs: &[(&str, &str)]) -> std::process::Output {
    let mut cmd = Command::new(binary());
    cmd.arg("health").args(args);
    for (k, v) in envs {
        cmd.env(k, v);
    }
    cmd.output().expect("ofm health failed to spawn")
}

#[test]
fn test_health_global_read_only_prints_report() {
    // The exit code is 0 on a clean machine and 1 when findings are present
    // (the test binary itself runs inside an ofm-managed worktree, so live
    // ofm instances may legitimately appear). Either way a report must print.
    let out = run_health(&["--global"]);
    let code = out.status.code();
    assert!(
        code == Some(0) || code == Some(1),
        "global read-only exits 0 (clean) or 1 (findings), got {code:?}"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("ofm health — global"),
        "global report should print a heading, got: {stdout}"
    );
}

#[test]
fn test_health_teardown_unknown_pid_exits_zero() {
    let out = run_health(&["--teardown", "999999"]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("no ofm instance for pid 999999"),
        "unknown/dead pid reports no instance, got: {stdout}"
    );
}

#[test]
fn test_health_local_against_temp_footprint() {
    let tmp = tempfile::TempDir::new().unwrap();
    let fp = tmp.path().to_str().unwrap();
    let out = run_health_with_env(&[], &[("OFM_FOOTPRINT", fp)]);
    assert_eq!(out.status.code(), Some(0), "local read-only exits 0");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("ofm health — local instance"),
        "local report should print a heading, got: {stdout}"
    );
}

#[test]
fn test_health_by_pid_teardown_lifecycle() {
    // Spawn a shell that inherits OFM_FOOTPRINT and records its PID in the
    // footprint's ofm.pid — simulating a leaked ofm-descended process from a
    // SIGKILLed instance.
    let tmp = tempfile::TempDir::new().unwrap();
    let fp = tmp.path().to_str().unwrap().to_string();

    use std::os::unix::process::CommandExt;
    let mut child = Command::new("sh")
        .arg("-c")
        .arg("sleep 300")
        .env("OFM_FOOTPRINT", &fp)
        .process_group(0)
        .spawn()
        .expect("spawn sh child");
    let pid = child.id();
    let pid_file = Path::new(&fp).join("ofm.pid");
    std::fs::write(&pid_file, pid.to_string()).unwrap();

    // Read-only check: the leaked shell is attributed to the footprint → exit 1.
    let out = run_health(&["--teardown", &pid.to_string()]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "read-only check reports findings (leaked shell)"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("ofm health — instance"),
        "by-pid report should print a heading, got: {stdout}"
    );

    // Teardown: kills the shell by exact PID, removes the pid-file → exit 0.
    let out = run_health(&["--do-teardown", &pid.to_string()]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "teardown should remove all ofm-descended resources, stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(
        !pid_file.exists(),
        "teardown must remove the footprint pid-file"
    );

    // The child is actually dead (pid_alive treats the zombie the teardown
    // leaves behind — which the test binary has yet to reap — as gone).
    assert!(
        !ofm::procscan::pid_alive(pid as i32),
        "test-owned shell must be dead after teardown"
    );

    // Reap the child handle so no zombie outlives the test.
    let _ = child.wait();
}

#[test]
fn test_health_usage_errors_exit_two() {
    // --global with --teardown <PID> is a usage error.
    let out = run_health(&["--global", "--teardown", "1"]);
    assert_eq!(out.status.code(), Some(2));

    // Unknown flag.
    let out = run_health(&["--frobnicate"]);
    assert_eq!(out.status.code(), Some(2));

    // Non-numeric PID.
    let out = run_health(&["--teardown", "abc"]);
    assert_eq!(out.status.code(), Some(2));
}
