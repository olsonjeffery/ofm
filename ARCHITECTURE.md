# Architecture

## Project Layout

```
ofm/
├── Cargo.toml
├── src/
│   ├── main.rs          # Entry point: DB init, migrations, rauthy, server
│   ├── lib.rs           # Module re-exports for integration tests
│   ├── config.rs        # OfmConfig, YAML + env var overlay
│   ├── logging.rs       # Tracing/logging init
│   ├── db/              # mod.rs (DDL, migrations), schema.rs (models)
│   ├── auth/            # OAuth/OIDC, JWKS, API keys, sessions
│   ├── server/          # Axum router, state, error, routes/, ws/
│   ├── webapp/          # Leptos SSR pages, islands, components
│   ├── providers/oh_my_pi/ # oh-my-pi: PTY spawn/reader, session management
│   ├── orchestration/   # State machine, guards, recovery, completion
│   ├── providers/       # LlmProvider trait, oh-my-pi/opencode providers
│   ├── agents/          # Prompt builders (planning, impl, review, PR)
│   ├── services/        # Auth, projects, tasks, settings, session, transcript
│   ├── archive/         # Task doc I/O, context prompt
│   ├── worktree/        # Git worktree management
│   ├── rauthy/          # Local rauthy lifecycle
│   └── cli/             # CLI subcommands
├── tests/               # 13 integration test files
├── templates/           # Agent prompt templates
└── assets/              # Bulma CSS, logos
```

The workspace has a single member crate (`ofm` binary) defined inline.

## Database

- **Engine**: [hiqlite](https://crates.io/crates/hiqlite) — async, Raft-capable embedded SQLite with built-in durability via WAL + auto-heal crash recovery. Single-node deployment eliminates the Mutex bottleneck in axum handlers.
- **Schema**: 15+ tables defined via raw SQL DDL in `src/db/mod.rs`. Project and task IDs use `INTEGER PRIMARY KEY AUTOINCREMENT`; other UUIDs (users, sessions, conversations) are stored as `TEXT`. Booleans are `INTEGER` (0/1), JSON as `TEXT`, and timestamps as ISO 8601 `TEXT` strings.
- **Migration system**: A `_migrations` tracking table records which migrations have been applied. Each migration is a named SQL DDL statement; only unapplied migrations execute on startup.

### Tables

| Table | Purpose |
|-------|---------|
| `users` | User accounts with OIDC auth |
| `projects` | Project definitions (repo paths, monorepo subproject paths) |
| `project_members` | Many-to-many user/project join table |
| `tasks` | Task definitions with workflow state flags |
| `conversations` | LLM conversation sessions (omp-mediated) |
| `task_agent_runs` | Agent execution tracking per task |
| `messages` | Transcript events (composite PK: project_key, session_id, seq) |
| `session_summaries` | Session memory snapshots (composite PK: project_key, session_id) |
| `app_settings` | Global key-value configuration store |
| `user_agent_model_settings` | Per-user agent/model configuration |
| `worktrees` | Worktree tracking table |
| `sessions` | OAuth session management |
| `user_model_configs` | User-specific model configuration |
| `agent_harness_configs` | Per-agent harness configuration |

## Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| tokio | 1 (full) | Async runtime |
| axum | 0.8 | Web framework with WS support |
| hiqlite | 0.13 | Async embedded SQLite (Raft-capable, WAL + auto-heal) |
| leptos | 0.7 | Webapp SSR framework (islands pattern) |
| pulldown-cmark | 0.11 | Markdown-to-HTML rendering |
| ammonia | 0.22 | HTML sanitization |
| portable-pty | 0.8 | Cross-platform PTY spawn for omp subprocess |
| clap | 4 | CLI argument parsing |
| serde | 1 (derive) | Serialization/deserialization |
| serde_json | 1 | JSON support |
| uuid | 1 (v4) | UUID generation |
| chrono | 0.4 (serde) | Timestamp types |
| reqwest | 0.12 | HTTP client (OIDC discovery, model listing) |
| jsonwebtoken | 9 | JWT verification for OIDC tokens |
| sha2 | 0.10 | SHA-256 hashing (API keys) |
| tower | 0.5 | Middleware infrastructure |
| tower-http | 0.6 | Axum middleware (cors, auth, etc.) |
| cookie | 0.18 | Session cookie management |
| rand | 0.8 | Random number generation |
| tracing | 0.1 | Structured logging |
| toml | 0.8 | YAML config deserialization |
| tokio-stream | 0.1 | Async stream utilities |

## Application Lifecycle

1. **Config**: Load `OfmConfig` from YAML file + env var overlay (`OFM_*`).
2. **Logging**: Initialize tracing/logging based on config.
3. **Database**: Start hiqlite node with `data_dir`, run pending migrations.
4. **Rauthy**: If `OFM_RAUTHY_ENABLED`, spawn rauthy via PTY, wait for health, configure reverse proxy at `/auth`. The container runs with the host user's UID via Docker's `--user` flag so files in the rauthy data directory are owned by the host user and cleanup does not require root.
5. **Server**: Start axum HTTP server with WebSocket support on configured `OFM_HOSTNAME:OFM_PORT`.
6. **WebSocket**: Accept connections, manage task subscriptions, stream agent events.
7. **oh-my-pi sessions**: Spawn `omp --mode rpc` subprocesses per turn, manage PTY lifecycle, stream events.
8. **Shutdown**: Graceful shutdown — stop accepting connections, kill subprocesses, stop rauthy, close DB.

## WebSocket Real-Time Bus

The server maintains a WebSocket hub for live UI updates. Clients subscribe to per-task channels. Events (streaming deltas, agent-run status changes, task-blocked signals) are broadcast to subscribers in real time. Subscription management handles reconnection and scoped interest sets (only the tasks currently visible on screen).

## WebApp (Leptos Islands)

All webapp UI follows the Islands Architecture pattern:
- The shell page is SSR-rendered via `leptos::ssr::render_to_string` from a plain axum handler.
- Each functional UI unit ("island") is a Leptos `#[component]` rendered to an HTML fragment by its own axum endpoint (`/webapp/islands/{name}`).
- A minimal inline JS runtime fetches islands and supports re-fetch via `[data-island-refresh]` buttons.
- Auth is enforced by the existing `AuthLayer` and per-handler `AuthUser` extractor.
- Styling via Bulma CSS with MDI icons.

## Agent Prompt Pipeline

Agent prompts are assembled from templates in `templates/`:
- `templates/planification.md` — planning prompt
- `templates/plan-template.md` — plan output format
- More prompt templates in `src/agents/` for implementation, review, and PR agents.

Prompt assembly functions in `src/agents/planning.rs` and related modules build the turn input from task context, model settings, and template rendering.

## YAML Config Overlay

Configuration is loaded from YAML files with environment variable overlay:
- Base config from `{footprint}/config/ofm.yml`
- Env vars with `OFM_` prefix override YAML values
- `OFM_FOOTPRINT` (default `~/.ofm`) derives all data paths (DB, archive, config, rauthy)
- `OFM_DB_PATH`, `OFM_ARCHIVE_ROOT`, `OFM_CONFIG` are eliminated in favor of footprint-derived paths

## Recurring Patterns

- **snake_case** naming for all columns and Rust identifiers
- **Custom error types** via `src/server/error.rs` — `AppError` enum with typed HTTP responses, replacing `Box<dyn Error>`
- **`TEXT` storage** for UUIDs (users, sessions, conversations, etc.), timestamps, and JSON values; project/task IDs use `INTEGER` (SQLite convention)
- **`AuthLayer` Tower middleware** for request authentication (JWT via JWKS, API key hash lookup)
- **`spawn_blocking`** for blocking I/O operations (PTY reads), sending events through `mpsc::Sender::blocking_send`

## Subprocess Invocation

The `omp` (oh-my-pi) binary is invoked as `omp --mode rpc` — the `--mode` flag
with value `rpc` is the correct CLI form. Do NOT use the positional `omp rpc`
form — that is incorrect. The spawn call in `src/providers/oh_my_pi/mod.rs` is the single
source of truth: `cmd.arg("--mode").arg("rpc")`.

## Design Decisions

- **hiqlite over rusqlite**: hiqlite provides an async, Raft-capable SQLite database with built-in durability via WAL + auto-heal crash recovery. Single-node deployment eliminates the Mutex bottleneck in axum handlers.
- **OIDC over password auth**: Production-ready authentication without implementing bespoke password handling. Supports enterprise SSO.
- **Embedded DB over client-server**: Eliminates external database infrastructure for development and small-scale deployments. hiqlite manages state files inside the configured `data_dir`.
- **Raw SQL DDL over migration framework**: DDL is wrapped in a simple `_migrations` tracking table, keeping the migration system self-contained.
- **WebSocket for live UI**: Real-time updates via WebSocket subscriptions instead of polling, enabling live agent-streaming and board state updates.
- **Leptos Islands over SPA**: Server-side rendered islands reduce client JS bundle and simplify auth (SSR handlers share server-side auth context without a separate token refresh for the SPA shell).
- **Dual harness**: Both `oh-my-pi` and `opencode` are first-class peers behind the `LlmProvider` trait abstraction.
- **Footprint-derived paths**: `OFM_FOOTPRINT` is the single root for all data directories, eliminating the env-var explosion of `OFM_DB_PATH`, `OFM_ARCHIVE_ROOT`, `OFM_CONFIG`.
