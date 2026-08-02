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
│   │   └── routes/
│   │       └── conversations.rs  # Chat API endpoints (Phase 2)
│   ├── webapp/          # Leptos SSR pages, islands, components
│   │   ├── pages/chat.rs       # Real-time chat view (Phase 4)
│   │   └── components/
│   │       ├── conversation_list.rs  # Conversation sidebar (Phase 5)
│   │       ├── message_stream.rs     # Streaming event display (Phase 5)
│   │       ├── chat_input.rs         # Manual message input (Phase 5)
│   ├── orchestration/   # State machine, guards, recovery, completion
│   ├── providers/       # LlmProvider trait, opencode_sdk-driven provider
│   ├── agents/          # Prompt builders (planning, impl, review, PR)
│   ├── services/        # Auth, projects, tasks, settings, session, transcript, export_import
│   ├── archive/         # Task doc I/O, context prompt
│   ├── worktree/        # Git worktree management
│   ├── ramalama/        # One-shot SLM queries via ramalama sub-process (phi4-mini)
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
| `conversations` | LLM conversation sessions (provider-agnostic via `provider_session_id`, renamed from `omp_session_id`) |
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
| leptos | 0.8 | Webapp SSR framework (islands pattern) |
| leptos_styling | 0.3 | Style sheet macro for Leptos |
| pulldown-cmark | 0.13 | Markdown-to-HTML rendering |
| ammonia | 4 | HTML sanitization |
| clap | 4 | CLI argument parsing |
| serde | 1 (derive) | Serialization/deserialization |
| serde_json | 1 | JSON support |
| serde_yaml | 0.9 | YAML config deserialization |
| uuid | 1 (v4) | UUID generation |
| chrono | 0.4 (serde) | Timestamp types |
| reqwest | 0.12 | HTTP client (OIDC discovery, model listing) |
| jsonwebtoken | 9 | JWT verification for OIDC tokens |
| sha2 | 0.10 | SHA-256 hashing (API keys) |
| tower | 0.5 | Middleware infrastructure |
| tower-http | 0.7 | Axum middleware (cors, fs, etc.) |
| cookie | 0.18 | Session cookie management |
| rand | 0.8 | Random number generation |
| tracing | 0.1 | Structured logging |
| tracing-subscriber | 0.3 | Logging subscriber with env-filter |
| tokio-stream | 0.1 | Async stream utilities |
| async-trait | 0.1 | Async trait support for LlmProvider |
| thiserror | 2 | Derive macro for error types |
| axum-extra | 0.10 | Cookie extraction/extensions |
| base64 | 0.22 | Base64 encoding for PKCE |
| url | 2 | URL parsing |
| hex | 0.4 | Hex encoding |

## Application Lifecycle

1. **Config**: Load `OfmConfig` from YAML file + env var overlay (`OFM_*`).
2. **Logging**: Initialize tracing/logging based on config.
3. **Database**: Start hiqlite node with `data_dir`, run pending migrations.
4. **Rauthy**: If `OFM_RAUTHY_ENABLED`, spawn rauthy as a Docker container via `tokio::process::Command`, wait for health at the container's direct port, and assign the instance to `_rauthy_instance` before any fallible step so `Drop` removes the container (`docker rm -f ofm-rauthy-<footprint-hash>`) on every failure path. The container always binds `0.0.0.0` (`-p 0.0.0.0:{port}:8080`; Docker only accepts IPs for the host bind interface, and `OFM_HOSTNAME` may be a non-IP hostname) and advertises `PUB_URL={OFM_HOSTNAME}:{port}`, so the browser reaches rauthy directly on that hostname. The container runs with the host user's UID via Docker's `--user` flag so files in the rauthy data directory are owned by the host user and cleanup does not require root. The footprint-derived container name is stable, so a stale container from a SIGKILLed instance is reaped by the startup `docker rm -f` on the next run of the same footprint.
5. **Server**: Start axum HTTP server with WebSocket support on configured `OFM_HOSTNAME:OFM_PORT`.
6. **WebSocket**: Accept connections, manage task subscriptions, stream agent events.
7. **OpenCode provider sessions**: Spawn `opencode serve` subprocesses per user in their own process groups, manage lifecycle, stream events. Teardown kills each spawned process group precisely (`kill(-pgid)`); the pool is drained on shutdown.
8. **Shutdown**: Graceful shutdown — stop accepting connections, kill subprocesses, remove the rauthy container by name, close DB.

## OAuth Access Token Cache

The `AppState` struct in `src/server/state.rs` contains an in-memory access token cache:
```rust
pub access_tokens: Arc<Mutex<HashMap<Uuid, String>>>,
```

Maps `user_id → OAuth access_token`. Populated on every token refresh:
- In the background startup task in `src/main.rs` (refreshes all stored sessions)
- In the `POST /api/auth/refresh` handler in `src/server/routes/auth.rs`

On successful refresh, the token is stored and a WS broadcast `"agent token refreshed"` is sent to all active conversation topics for that user.

### Expiry-Aware Cache Access

The `get_or_refresh_token` helper in `src/orchestration/mod.rs` centralizes cache access with JWT expiry validation and OIDC refresh fallback. Both `start_next_agent` and the `send_message` handler use it.

Access pattern:
1. Check cache — if the cached token's `exp` claim is in the past, treat as cache miss
2. On miss, if OIDC provider is configured, query the full session row (including persisted `access_token`) from the DB
3. If the persisted `access_token` is non-empty, call `validate_access_token` against the OIDC `userinfo_endpoint`:
   - Valid (200) → seed cache with persisted token, return
   - Invalid (401) or network error → fall through to step 4
4. Call `auth::refresh_access_token` to obtain a fresh token:
   - On success → seed cache, persist new `access_token` to DB, return
   - On `JwtToken` error → retry once after 500ms backoff:
     - Retry success → cache + persist + return
     - Retry fail → delete session, return `None`
   - On `invalid_grant`/`invalid_token` → session already deleted by refresh function, return `None`
5. On all other failures, log `tracing::error!` and return `None` (caller skips `.ofm_agent.json` write)

### `.ofm_agent.json` Write Guards

The `write_ofm_agent_json` function in `src/orchestration/mod.rs` guards against writing invalid tokens:
- **Empty token check**: returns early with `tracing::error!` if `access_token` is empty
- **Expired token check**: decodes the JWT `exp` claim via `decode_jwt_exp`; returns early with `tracing::error!` if the token has expired past `chrono::Utc::now()`

### Session Cookie Clearing on Invalidated Refresh

When `refresh_access_token` receives `invalid_grant` or `invalid_token` from the OIDC provider, the session row is deleted from the DB. The `POST /api/auth/refresh` handler in `src/server/routes/auth.rs` now catches the error and clears the `ofm_session` cookie by calling `jar.remove(Cookie::from("ofm_session"))` before returning a 400 response with `"session expired, please re-authenticate"`.

## `external_directory` Allowlist

The opencode server's config includes an `external_directory` permission that controls
which filesystem paths agent tools (read, edit, write, glob, grep, bash) can access.
This was previously set to `"allow"` (unrestricted), which allowed agents to write
anywhere on the filesystem — including the main git repository outside their task
worktree.

As of Task 185, `external_directory` is an object allowlisting three path patterns:

```
{footprint}/worktrees/**   → allow  (task worktrees — read+write)
{footprint}/archive/**     → allow  (task docs, spec files — read+write)
/tmp/**                    → allow  (scratch/temp files — read+write)
```

Everything else is blocked. This is configured at server-start-time in three places:

1. **`src/opencode_sdk/pool.rs:build_server_config()`** — used when spawning pooled
   opencode servers via `OpenCodeServerPool::spawn_entry()`. Receives the footprint
   from the provider, which received it from `orchestration/mod.rs`.
2. **`src/providers/opencode_sdk_provider.rs:build_server_config()`** — used for
   transient servers (model listing, one-shot prompts). Falls back to `"allow"`
   when no footprint is available.
3. **`src/opencode_sdk/server.rs:create_opencode_server()`** — fallback default
   config when `ServerOptions.config` is `None`. Falls back to `"allow` when no
   footprint is provided.

### Footprint plumbing chain

The footprint value (`OFM_FOOTPRINT`) is plumbed through the provider chain:

1. `orchestration/mod.rs:start_next_agent()` passes `footprint` to
   `registry::resolve_provider_for_user()`.
2. `providers/registry.rs:resolve_provider_for_user()` passes it to
   `OpenCodeSdkProvider::new()`.
3. `OpenCodeSdkProvider` stores it as `self.footprint: PathBuf`.
4. `OpenCodeSdkProvider::start()` passes `&self.footprint` to
   `OpenCodeServerPool::get_or_spawn()`.
5. `get_or_spawn()` → `spawn_entry()` → `build_server_config()` templates the
   three-path allowlist.

### Hardcoded `/tmp` fix

Previously, `provider.start()` was called with `Path::new("/tmp")` *before* the
worktree path was resolved from the database. This caused the opencode session's
working directory to be `/tmp` instead of the correct worktree path. The fix moves
`provider.start()` to *after* worktree resolution, using the correct worktree path.
See `src/orchestration/mod.rs`.

## OFM Context Prompt Injection

The `build_context_prompt()` in `src/archive/mod.rs` appends an "OFM Environment" section to every agent turn's prompt. This section describes the `.ofm_agent.json` file structure and provides `jq`-based instructions for reading connection details and deriving the task ID from the worktree directory name. The OAuth access token is **not** embedded in the prompt text — instead it is written to `.ofm_agent.json` in the worktree root (with `0600` permissions) as part of a structured JSON object. Agents parse the file with `jq` and pass the access token in `Authorization: Bearer` headers when calling the `agent-flags` endpoints.

The `.ofm_agent.json` file has the following schema:
```json
{
  "agentVars": {
    "accessToken": "...",
    "tokenExpiration": 1234567890,
    "ofmHost": "127.0.0.1",
    "ofmPort": 3183,
    "ofmPid": 12345
  }
}
```

### Session Directory Routing

The opencode server subprocess is spawned once per user (pooled) without a task-specific CWD — one
shared server cannot serve multiple worktrees. Instead, the provider threads the task worktree path as a
`directory` **query param** on every workspace-scoped HTTP call. `OpenCodeSdkProvider::start()` re-scopes
the pooled client handle via `OpencodeClient::with_directory(&worktree)` (`Arc::make_mut` clones the shared
inner for this provider instance only), and `src/opencode_sdk/client.rs` appends `?directory=<worktree>` to
`session.create`, `event.subscribe` (initial request **and** SSE reconnect), `session.prompt`, `session.prompt_async`,
and `session.abort`. This mirrors the reference implementation's `WorkspaceRoutingMiddleware`, which resolves
the workspace directory per HTTP call and falls back to the server's `process.cwd()` when absent — so a
session-row PATCH is insufficient and is no longer performed.

### File Recreation on Resume

In `src/server/routes/conversations.rs`, the `send_message()` handler recreates `.ofm_agent.json`
with a fresh access token and expiration timestamp before resuming a provider turn. This ensures
the agent always receives up-to-date credentials, even if the token expired between turns.

## Shared Agent Run Starting

The `start_next_agent()` function in `src/orchestration/mod.rs` consolidates all agent-run startup logic (config resolution, guard checks, session creation, provider startup, context-prompt building, turn initiation, and broadcast task spawning). Both the HTTP handler (`post_create_agent_run` in `src/server/routes/agent_runs.rs`) and auto-advancement callers use this shared function.

## Auto-Advancement Wiring

When an agent run completes and `completion_handler` returns `NextAction::StartAgent`, the broadcast task (in both `start_next_agent()` and the conversations broadcast task in `send_message()`) spawns a new task that calls `start_next_agent()` for the next phase. This wires auto-advancement through the implementation → review → PR pipeline without requiring the agent to signal completion via echo or curl (though curl-based flag endpoints are still available for specific phases like `complete-workflow` and `block-workflow`).

## WebSocket Real-Time Bus

The server maintains a WebSocket hub for live UI updates. Clients subscribe to per-task channels. Events (streaming deltas, agent-run status changes, task-blocked signals) are broadcast to subscribers in real time. Subscription management handles reconnection and scoped interest sets (only the tasks currently visible on screen).

## Real-Time Chat

The chat view (`/webapp/projects/{project_id}/tasks/{task_id}/chat`) provides a real-time conversation interface:

- **Event broadcasting**: When `post_create_agent_run` starts an agent turn, it calls `provider.start_turn(input)` which returns an `mpsc::Receiver<ProviderEvent>`. A background task reads events, persists them via `transcript::persist_event()`, maps `ProviderEvent` → `ServerMessage::Event`, and broadcasts via `ws_bus` under the task's `WsTopic`. On `Done`, it calls `completion_handler` to advance the state machine.
- **Tool event merging (server-side)** (Task 67): When a `ToolUse` with `result: Some(...)` arrives in the broadcast loop, instead of inserting a new row, the loop calls `transcript::update_tool_event()` to update the existing ToolUse row in-place with the completed input and output. The broadcast uses the `tool_updated` WS event type. This ensures the DB reflects the complete tool snapshot and page reloads show unified tool cards.
- **Provider-agnostic**: The broadcast task consumes `mpsc::Receiver<ProviderEvent>`, staying completely trait-agnostic. `OpenCodeSdkProvider` is the sole built-in provider.
- **Manual chat**: `POST /api/tasks/{task_id}/conversations/{id}/messages` — persists the user message and broadcasts `user_text` exactly once, then delegates to the `resume_or_recreate` helper. The helper loads the transcript, resumes (or, on failure, recreates) the provider turn, and spawns the broadcast task for the response events. It never re-persists or re-broadcasts the user message, which previously produced duplicate blue `.message-user` elements during live streaming (Task 81).
- **Orchestrator phase skip**: When a phase's agent config is missing (no model configured), `post_create_agent_run` creates a `Blocked` run and returns immediately. `next_agent()` checks config statuses before returning `StartAgent`, skipping unconfigured phases.
- **Chat API**: `GET /api/tasks/{task_id}/conversations` lists conversations with their associated runs; `GET /api/tasks/{task_id}/conversations/{id}` returns a conversation with its full message transcript.
- **UI Components**: The chat page has three Leptos SSR components — `ConversationList` (sidebar), `MessageStream` (event display with `overflow-wrap: break-word` bounding), and `ChatInput` (message input — agent-type phases dropdown removed in Task 204). The `AgentRunBanner` was removed in Task 2 (notification bar replaced with task detail page's Agents box controls).

## WebApp (Leptos Islands)

All webapp UI follows the Islands Architecture pattern:
- The shell page is SSR-rendered via `leptos::ssr::render_to_string` from a plain axum handler.
- Each functional UI unit ("island") is a Leptos `#[component]` rendered to an HTML fragment by its own axum endpoint (`/webapp/islands/{name}`).
- A minimal inline JS runtime fetches islands and supports re-fetch via `[data-island-refresh]` buttons.
- Auth is enforced by the existing `AuthLayer` and per-handler `AuthUser` extractor.
- Styling via Bulma CSS with MDI icons.

### UI Components

- **Breadcrumbs**: Shared breadcrumb navigation system. `BreadcrumbItem` data struct holds `title`, `icon`, and `path`. A `breadcrumb_registry` module centralizes canonical breadcrumb definitions (e.g., `all_projects()`, `project()`, `task()`, `chat()`, `settings()`). The `Breadcrumbs` Leptos component renders Bulma `<nav class="breadcrumb">` markup. Breadcrumbs flow from page handler -> `render_shell()` -> `ShellPage` -> `Navbar`, appearing immediately after the WS status indicator in the navbar-start div.

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

## Design Decisions

- **hiqlite over rusqlite**: hiqlite provides an async, Raft-capable SQLite database with built-in durability via WAL + auto-heal crash recovery. Single-node deployment eliminates the Mutex bottleneck in axum handlers.
- **OIDC over password auth**: Production-ready authentication without implementing bespoke password handling. Supports enterprise SSO.
- **Embedded DB over client-server**: Eliminates external database infrastructure for development and small-scale deployments. hiqlite manages state files inside the configured `data_dir`.
- **Raw SQL DDL over migration framework**: DDL is wrapped in a simple `_migrations` tracking table, keeping the migration system self-contained.
- **WebSocket for live UI**: Real-time updates via WebSocket subscriptions instead of polling, enabling live agent-streaming and board state updates.
- **Leptos Islands over SPA**: Server-side rendered islands reduce client JS bundle and simplify auth (SSR handlers share server-side auth context without a separate token refresh for the SPA shell).
- **Single harness**: `opencode` is the built-in provider behind the `LlmProvider` trait abstraction, backed by the `opencode_sdk` submodule.
- **Footprint-derived paths**: `OFM_FOOTPRINT` is the single root for all data directories, eliminating the env-var explosion of `OFM_DB_PATH`, `OFM_ARCHIVE_ROOT`, `OFM_CONFIG`.
