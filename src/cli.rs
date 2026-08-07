//! `ofm health` CLI sub-command.
//!
//! This is OFM's first CLI (`main()` never previously read `argv`). All
//! process introspection + teardown lives in [`crate::procscan`]; this module
//! only parses args, dispatches, renders markdown, and owns the exit-code
//! contract:
//!
//! - `0` = clean (read-only) / fully torn down
//! - `1` = findings present (read-only)
//! - `2` = usage/internal error
//! - `3` = teardown left survivors

use std::path::PathBuf;

use crate::config::OfmConfig;
use crate::procscan::{self, ClassKind, GlobalReport, RestartStatus};

#[derive(Debug, PartialEq, Eq)]
pub enum HealthArgs {
    Local,
    ByPid { pid: i32, do_teardown: bool },
    Global { do_teardown: bool },
}

/// Parse `ofm health ...` arguments (everything after the `health` token).
///
/// Accepts: bare `ofm health`; `--teardown <PID>`; `--do-teardown <PID>`;
/// `--global`; `--global --do-teardown`. Rejects `--global` +
/// `--teardown <PID>`, `--do-teardown` without a target, unknown flags and
/// non-numeric PIDs.
pub fn parse_args(args: &[String]) -> Result<HealthArgs, String> {
    let mut global = false;
    let mut by_pid: Option<i32> = None;
    let mut do_teardown = false;

    let mut iter = args.iter().peekable();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--global" => {
                if by_pid.is_some() {
                    return Err("--global cannot be combined with --teardown <PID>".into());
                }
                global = true;
            }
            "--teardown" | "--do-teardown" => {
                let teardown_flag = arg == "--do-teardown";
                if global {
                    if !teardown_flag {
                        return Err("--teardown <PID> cannot be combined with --global".into());
                    }
                    do_teardown = true;
                } else if teardown_flag && iter.peek().map(|s| s.as_str()) == Some("--global") {
                    // `--do-teardown --global` — teardown targets the machine.
                    iter.next();
                    global = true;
                    do_teardown = true;
                } else {
                    let pid_str = iter
                        .next()
                        .ok_or_else(|| format!("missing PID for {arg}"))?;
                    let pid: i32 = pid_str
                        .parse()
                        .map_err(|_| format!("invalid PID: {pid_str}"))?;
                    if pid <= 0 {
                        return Err(format!("invalid PID: {pid_str}"));
                    }
                    if by_pid.is_some() {
                        return Err("duplicate --teardown PID".into());
                    }
                    by_pid = Some(pid);
                    do_teardown = teardown_flag;
                }
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    if global {
        Ok(HealthArgs::Global { do_teardown })
    } else if let Some(pid) = by_pid {
        Ok(HealthArgs::ByPid { pid, do_teardown })
    } else if do_teardown {
        Err("--do-teardown requires --global or --teardown <PID>".into())
    } else {
        Ok(HealthArgs::Local)
    }
}

/// Dispatch entry point. `args` is the full argv (minus the binary), i.e. it
/// includes the leading `health` token.
pub async fn run(args: Vec<String>) -> i32 {
    let rest: Vec<String> = if args.first().map(String::as_str) == Some("health") {
        args[1..].to_vec()
    } else {
        args
    };
    match parse_args(&rest) {
        Ok(HealthArgs::Local) => run_local().await,
        Ok(HealthArgs::ByPid { pid, do_teardown }) => run_by_pid(pid, do_teardown).await,
        Ok(HealthArgs::Global { do_teardown }) => run_global(do_teardown).await,
        Err(e) => {
            eprintln!("ofm health: usage error: {e}");
            2
        }
    }
}

async fn run_local() -> i32 {
    let cfg = OfmConfig::load();
    let footprint = &cfg.footprint;
    let classified = procscan::scan_classified();
    let pid_file_pid = procscan::read_pid_file(footprint);
    let guard = procscan::restart_guard(&classified, footprint, std::process::id(), pid_file_pid);
    let own = procscan::collect_for_footprint(&classified, footprint);

    let mut out = String::new();
    out.push_str("# ofm health — local instance\n\n");
    out.push_str(&format!("- footprint: `{footprint}`\n"));
    out.push_str(&format!("- current pid: {}\n", std::process::id()));
    match &guard {
        RestartStatus::Blocked(live) => {
            out.push_str(&format!(
                "- ✖ another ofm instance (pid {live}) owns this footprint\n"
            ));
        }
        RestartStatus::Dirty(stragglers) => {
            out.push_str("- ▲ leftover ofm resources found:\n");
            out.push_str(&procscan::render(stragglers));
        }
        RestartStatus::Clean => out.push_str("- ✔ footprint is clean\n"),
    }
    if own.iter().any(|c| c.kind == ClassKind::Ofm) {
        out.push_str("- ✔ ofm is running\n");
    }
    for c in own.iter().filter(|c| c.kind != ClassKind::Ofm) {
        out.push_str(&format!(
            "- pid {} ({}) — {:?}\n",
            c.info.pid, c.info.comm, c.kind
        ));
    }
    println!("{out}");
    if matches!(guard, RestartStatus::Clean) {
        0
    } else {
        1
    }
}

async fn run_by_pid(pid: i32, do_teardown: bool) -> i32 {
    let home = std::env::var("HOME").map(PathBuf::from).unwrap_or_default();
    let cwd = std::env::current_dir().unwrap_or_default();
    let pid_files = procscan::find_pid_files(&home, &cwd);
    let scan = procscan::scan();
    let classified: Vec<_> = scan.iter().map(procscan::classify).collect();

    let Some(target) = procscan::resolve_instance(&scan, pid, &pid_files) else {
        println!("no ofm instance for pid {pid}");
        return 0;
    };

    if !do_teardown {
        let stragglers: Vec<_> = match target.footprint.as_deref() {
            Some(fp) => procscan::collect_for_footprint(&classified, fp),
            None => Vec::new(),
        };
        let mut out = String::new();
        out.push_str(&format!("# ofm health — instance {pid}\n\n"));
        match &target.live {
            Some(_) => out.push_str(&format!(
                "- ✔ ofm is running (pid {pid}, footprint {:?})\n",
                target.footprint
            )),
            None => out.push_str(&format!(
                "- ✖ ofm pid {pid} is not running (attributed via `ofm.pid`, footprint {:?})\n",
                target.footprint
            )),
        }
        for c in &stragglers {
            out.push_str(&format!(
                "- pid {} ({}) — {:?}\n",
                c.info.pid, c.info.comm, c.kind
            ));
        }
        if stragglers.is_empty() && target.live.is_none() {
            out.push_str("- ✔ no leftover ofm-descended resources\n");
        }
        println!("{out}");
        if stragglers.is_empty() {
            0
        } else {
            1
        }
    } else {
        let result = procscan::teardown_footprint(&target, &classified);
        let mut out = String::new();
        out.push_str(&format!("# ofm teardown — instance {pid}\n\n"));
        out.push_str(&format!("- ofm roots killed: {}\n", result.ofm_killed));
        out.push_str(&format!(
            "- opencode server groups killed: {}\n",
            result.opencode_groups
        ));
        out.push_str(&format!("- shells killed: {}\n", result.shells_killed));
        out.push_str(&format!(
            "- rauthy spawners killed: {}\n",
            result.rauthy_spawners_killed
        ));
        out.push_str(&format!(
            "- rauthy containers removed: {}\n",
            result.containers_removed
        ));
        if result.survivors.is_empty() {
            out.push_str("- ✔ all ofm-descended resources gone\n");
        } else {
            out.push_str("- ✘ teardown left survivors:\n");
            out.push_str(&procscan::render(&result.survivors));
        }
        println!("{out}");
        if result.survivors.is_empty() {
            0
        } else {
            3
        }
    }
}

async fn run_global(do_teardown: bool) -> i32 {
    let classified = procscan::scan_classified();
    let containers = procscan::list_rauthy_containers();
    let report = procscan::collect_global(&classified, &containers);

    if !do_teardown {
        print_global_report(&report);
        let findings = report.ofm_instances.len()
            + report.opencode.len()
            + report.rauthy_spawners.len()
            + report.containers.len()
            + report.shells.len();
        if findings == 0 {
            0
        } else {
            1
        }
    } else {
        let result = procscan::teardown_global(&report);
        let mut out = String::new();
        out.push_str("# ofm teardown — global\n\n");
        out.push_str(&format!("- ofm roots killed: {}\n", result.ofm_killed));
        out.push_str(&format!(
            "- opencode server groups killed: {}\n",
            result.opencode_groups
        ));
        out.push_str(&format!(
            "- rauthy spawners killed: {}\n",
            result.rauthy_spawners_killed
        ));
        out.push_str(&format!("- shells killed: {}\n", result.shells_killed));
        out.push_str(&format!(
            "- rauthy containers removed: {}\n",
            result.containers_removed
        ));
        if !result.containers_remaining.is_empty() {
            out.push_str(&format!(
                "- remaining ofm-rauthy containers: {}\n",
                result.containers_remaining.len()
            ));
        }
        if result.survivors.is_empty() {
            out.push_str("- ✔ all ofm-descended resources gone\n");
        } else {
            out.push_str("- ✘ teardown left survivors:\n");
            out.push_str(&procscan::render(&result.survivors));
        }
        println!("{out}");

        // Remove now-dead pid-file markers left by SIGKILLed instances.
        let home = std::env::var("HOME").map(PathBuf::from).unwrap_or_default();
        let cwd = std::env::current_dir().unwrap_or_default();
        for (path, pid) in procscan::find_pid_files(&home, &cwd) {
            if !procscan::pid_alive(pid) {
                let _ = std::fs::remove_file(path);
            }
        }

        if result.survivors.is_empty() && result.containers_remaining.is_empty() {
            0
        } else {
            3
        }
    }
}

fn print_global_report(report: &GlobalReport) {
    let mut out = String::new();
    out.push_str("# ofm health — global\n\n");
    out.push_str(&format!(
        "- ofm instances: {}\n",
        report.ofm_instances.len()
    ));
    for c in &report.ofm_instances {
        out.push_str(&format!(
            "- pid {} (footprint {:?})\n",
            c.info.pid, c.footprint
        ));
    }
    out.push_str(&format!("- opencode servers: {}\n", report.opencode.len()));
    for c in &report.opencode {
        out.push_str(&format!("- pid {} (owned group)\n", c.info.pid));
    }
    out.push_str(&format!(
        "- rauthy spawners: {}\n",
        report.rauthy_spawners.len()
    ));
    for c in &report.rauthy_spawners {
        out.push_str(&format!(
            "- pid {} (container {:?})\n",
            c.info.pid, c.container
        ));
    }
    out.push_str(&format!(
        "- rauthy containers: {}\n",
        report.containers.len()
    ));
    for n in &report.containers {
        out.push_str(&format!("- {n}\n"));
    }
    out.push_str(&format!("- shells: {}\n", report.shells.len()));
    out.push_str(&format!("- total rss: {} KB\n", report.total_rss_kb));
    println!("{out}");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_parse_args_acceptance() {
        assert_eq!(parse_args(&args(&[])).unwrap(), HealthArgs::Local);
        assert_eq!(
            parse_args(&args(&["--teardown", "1234"])).unwrap(),
            HealthArgs::ByPid {
                pid: 1234,
                do_teardown: false
            }
        );
        assert_eq!(
            parse_args(&args(&["--do-teardown", "1234"])).unwrap(),
            HealthArgs::ByPid {
                pid: 1234,
                do_teardown: true
            }
        );
        assert_eq!(
            parse_args(&args(&["--global"])).unwrap(),
            HealthArgs::Global { do_teardown: false }
        );
        assert_eq!(
            parse_args(&args(&["--global", "--do-teardown"])).unwrap(),
            HealthArgs::Global { do_teardown: true }
        );
        assert_eq!(
            parse_args(&args(&["--do-teardown", "--global"])).unwrap(),
            HealthArgs::Global { do_teardown: true }
        );
    }

    #[test]
    fn test_parse_args_rejection() {
        // --global + --teardown <PID>
        assert!(parse_args(&args(&["--global", "--teardown", "1"])).is_err());
        // --do-teardown without a target
        assert!(parse_args(&args(&["--do-teardown"])).is_err());
        // unknown flag
        assert!(parse_args(&args(&["--frobnicate"])).is_err());
        // non-numeric PID
        assert!(parse_args(&args(&["--teardown", "abc"])).is_err());
        // non-positive PID
        assert!(parse_args(&args(&["--teardown", "-5"])).is_err());
        // missing PID
        assert!(parse_args(&args(&["--teardown"])).is_err());
        // duplicate PIDs
        assert!(parse_args(&args(&["--teardown", "1", "--teardown", "2"])).is_err());
    }
}
