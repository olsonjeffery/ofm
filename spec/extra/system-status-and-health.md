# System Status & Health

## Purpose

As `ofm` grows more complex there is a pronounced need to monitor the
sub-systems hanging off of it while it runs: the binaries it depends on, the
sub-processes it manages (`opencode serve` pool, rauthy container + `docker
run` spawner, the embedded hiqlite cluster), external dependent services
(`gh` auth, the OIDC provider), and the persistence layer.

This feature adds:

- a backend **System Health / Dependencies Check** (bin detection + live
  subsystem probes);
- a rolling, append-only **`system-health-entry`** table giving both the
  latest state per resource and a time series for agent-consumed mermaid
  charts;
- three delivery modes: a startup **console report**, a markdown + JSON
  **System Status page** (`/webapp/system`, reachable from the navbar agents
  dropdown), and **JSON + a WS `system_status` event** (drives the navbar
  running-services badge);
- the **`ofm health` CLI** for process introspection and precise teardown;
- a **`system-status` capability** (an OAuth-scope group) gating agent-session
  injection of System Status & Health data.

## Dependency check

`src/services/system_health.rs` keeps a code-based heterogeneous list
(`BINS`, a `BinSpec` slice):

| key | bin | required when | notes |
|---|---|---|---|
| `git` | `git` | always | plus `git-lfs` (informational) |
| `opencode` | `opencode` | always | |
| `bash` | `bash` → `sh` fallback | always | |
| `npm` | `npm` | always | plus `npx`, `node` (informational) |
| `docker` | `docker` | **iff** `rauthy_enabled` | |
| `gh` | `gh` | never | reported-but-non-fatal |
| `rustup` | `rustup` | never | plus `cargo` |
| `playwright-cli` | `playwright-cli` | never | browser install list added as metadata |
| `rtk` | `rtk` | never | |

Probing is hand-rolled (no new crates): `find_in_path` splits `PATH`,
`read_tool_version` runs `<bin> --version` and parses the first token,
`install_method` classifies `cargo`/`npm`/`system`/`other` from the path, and
`playwright_browsers` lists `PLAYWRIGHT_BROWSERS_PATH` (default
`~/.cache/ms-playwright`). Missing bins are reported with status `missing`;
`bin_required` decides whether a missing bin aborts startup. `main` runs the
check **early** (after migrations, before rauthy/opencode start) purely to
make the required-dep crash decision (`exit 1`), then re-runs/persists the
full report after the server is up.

## Live system health

`live_health_check` snapshots, on an interval (default 30 s,
`DEFAULT_REFRESH_INTERVAL_MS`):

- **`live:opencode-pool`** — per-user `OpenCodeServerPool` entries: PID, port,
  RSS (`/proc/{pid}/statm`), count by status.
- **`live:rauthy`** (or **`live:oauth`**) — probes `http://127.0.0.1:{port}/
  health` (or the OIDC discovery URL when rauthy is disabled), recording HTTP
  status + latency; rauthy PID via `docker inspect` and RAM via `/proc`.
  The actual rauthy port is recorded in a process-wide
  `system_health::RAUTHY_PORT` (the configured `rauthy_port` may be `0` =
  random) so the background monitor never borrows `AppState`.
- **`live:hiqlite`** — `is_healthy_db`, `is_leader_db`, serialized
  `metrics_db` (Raft metrics), footprint size + **last-flush approximation**
  (mtime of the newest file under `{footprint}/hiqlite`; hiqlite 0.14 has no
  flush API without the disabled `backup` feature).
- **`live:gh`** — `gh api user` (timeout-guarded); `login`/`name` in
  metadata, `error` status on failure.

Every successful probe stamps `metadata.last_interaction`.

## `system-health-entry` table

Rolling append-only log (migration `create_system_health_entry`):

```sql
CREATE TABLE system_health_entry (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    category TEXT NOT NULL,          -- 'dependency' | 'live'
    resource TEXT NOT NULL,          -- 'bin:git', 'live:opencode-pool', ...
    status TEXT NOT NULL,            -- 'ok' | 'warn' | 'missing' | 'error' | 'unknown'
    detail TEXT NOT NULL DEFAULT '',
    metadata TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT ''
);
CREATE INDEX idx_system_health_resource ON system_health_entry(resource, created_at);
```

Each refresh inserts fresh rows and prunes to the newest
`MAX_ROWS_PER_PRUNE` (500). Latest-state-per-resource =
`WHERE id IN (SELECT MAX(id) ... GROUP BY resource) ORDER BY resource`
(`latest_report`); the rolling history is `ORDER BY id DESC` (`history_report`,
limit capped at `HISTORY_LIMIT` = 1000) — enough for an agent to render a
mermaid `timeline`/`stateDiagram`.

## Delivery modes

1. **Console** — after the server is up, `main` persists the combined
   dependency + live report and prints it via `tracing::info!`
   (`render_markdown`).
2. **Page / markdown** — `GET /webapp/system` renders `render_markdown`
   through `<MarkdownViewer>` plus a "Live Data" card grid from
   `GET /api/system/status`; unicode status icons (`✔ ▲ ✖ ✘ –`) pass through
   ammonia sanitization. Timestamps carry `data-utc` attributes for
   `OfmTime.apply()` localization. The page is for any authenticated user; a
   muted note explains agent access requires the `system-status` capability.
3. **JSON / WS** — `GET /api/system/status` returns
   `{ generated_at, running_services, entries[] }`; `GET /api/system/history`
   returns rolling rows. The background monitor broadcasts a
   `system_status` event on the System topic with `{ running_services }`
   (`orchestration::broadcast_system_status`); the navbar badge
   (`SystemHealthBadge`) subscribes and re-fetches `/api/system/status`.

## `system-status` capability

A built-in OAuth-scope group named `system-status` (`is_oauth_scope = 1`) is
seeded at startup (`ensure_system_status_group`, mirroring
`ensure_admins_group`). Users whose `users.scopes` contains `system-status`
hold read-only membership (`groups::users_with_scope`); admins have implicit
access (`has_group_access`). `user_can_use_system_health` gates only
**agent-session injection**: in `start_next_agent`
(`src/orchestration/mod.rs`) and the task-detail preview (`GET /api/tasks/{id}`
in `src/server/routes/tasks.rs`), a capable user's context prompt is extended
with a `## System Health` markdown section (`render_agent_section`): the
current snapshot table plus a compact per-resource time series.

The page and API endpoints are **not** capability-gated (any authenticated
user may view the report); the Settings-dropdown "System" item is admin-only
(visible to `is_admin`), while the agents-dropdown "System Status" link is for
everyone.

## `ofm health` CLI

`ofm` previously had no CLI (`main()` never read `argv`). The `health`
sub-command (`src/cli.rs`, dispatched early in `main`) performs process
introspection + precise teardown (`src/procscan.rs`).

### Sub-commands

| Command | Behavior |
|---|---|
| `ofm health` | Local instance report: footprint, pid, restart-guard state, footprint-attributed processes |
| `ofm health --teardown <PID>` | Read-only check of the instance owning `PID` (live or dead-via-`ofm.pid`) |
| `ofm health --do-teardown <PID>` | Precise teardown of that instance's ofm-descended resources |
| `ofm health --global` | Machine-wide read-only report |
| `ofm health --global --do-teardown` | Machine-wide teardown |

### Exit-code contract

- `0` = clean (read-only) / fully torn down
- `1` = findings present (read-only)
- `2` = usage/internal error
- `3` = teardown left survivors

### Attribution + restart guard

- **Attribution key = `OFM_FOOTPRINT` in `/proc/{pid}/environ`.** Children
  inherit it (`opencode serve`, rauthy's `docker run`, shells launched by ofm).
- **`{footprint}/ofm.pid`** bridges dead-instance attribution: written at
  startup (0o600), removed on clean shutdown, left behind on SIGKILL, so a
  dead PID can still be attributed to a footprint.
- **Restart guard** (`procscan::restart_guard`, run in `main` before the DB
  starts): `Blocked(pid)` when a live ofm owns the footprint (or the pid-file
  pid is alive) → abort with a pointer to
  `ofm health --do-teardown <pid>`; `Dirty(stragglers)` when no live ofm but
  leftover opencode/shell/rauthy-spawner resources remain → report and
  precisely clean them; `Clean` otherwise.

### Precise-teardown safety rules (AGENTS.md, non-negotiable)

- **Never bulk-kill** (`pkill`/`killall`/`grep`+`kill`). Teardown is precise:
  - exact-PID kill: `SIGTERM` → grace (3 s) → `SIGKILL`;
  - process-group kill **only for groups OFM created** — `opencode serve` runs
    `process_group(0)`, so `kill(-pid, ...)`;
  - named-container removal **only** for `ofm-rauthy-<fnv64>` containers
    (`docker rm -f`), never other containers;
  - rauthy's `docker run` spawner is not in its own process group — exact PID
    only.
- Any process attributed to the footprint (including `Other` comms such as a
  `sleep` that `sh -c` `exec`'d into) is killed by exact PID.

## Startup order (fail fast, print late)

1. `health` sub-command dispatch (if argv[0] == `health`).
2. Config load → logging init → **restart guard** (block/clean stragglers).
3. DB setup + `ofm.pid` write.
4. Migrations, default user, `admins` + `system-status` groups, static prompts,
   orphan recovery.
5. **Early blocking dependency check** (missing required → `exit 1`).
6. Auth/rauthy/opencode/server bring-up; rauthy port recorded for health.
7. Post-ready: first live snapshot + persistence + console report; background
   monitor task (interval refresh + `system_status` broadcast), aborted on
   shutdown before the DB client closes.
8. Clean shutdown: pool teardown → `ofm.pid` removal → `client.shutdown()`.

## Testing

- Unit: `src/services/system_health.rs` (detection, renderers, prune,
  capability), `src/procscan.rs` (classify/attribution/restart_guard),
  `src/cli.rs` (`parse_args` matrix), `src/opencode_sdk/pool.rs` snapshot,
  `src/rauthy/mod.rs` container-name determinism, `src/webapp/components/
  navbar.rs` gating.
- Integration: `tests/system_health_test.rs` (status/history endpoints,
  persistence/prune, capability), `tests/webapp_test.rs` page render,
  `tests/cli_test.rs` via `CARGO_BIN_EXE_ofm` (global scan, dead PID,
  by-PID teardown lifecycle, local report).
- Playwright: `tests/playwright/system_status.spec.ts` (navbar badge,
  dropdown gating, page render, data-utc localization) + manual startup-crash,
  `ofm health` lifecycle, and agent-capability spot checks.
