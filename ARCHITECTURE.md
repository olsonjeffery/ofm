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
│   ├── server/          # Axum router, state, error, routes/, ws/, proxy/
│   │   ├── proxy.rs     # `/auth` reverse proxy to the embedded rauthy container
│   │   └── routes/
│   │       └── conversations.rs  # Chat API endpoints (Phase 2)
│   ├── webapp/          # Leptos SSR pages, islands, components
│   │   ├── pages/chat.rs       # Real-time chat view (Phase 4)
│   │   ├── pages/prompts.rs    # Prompt Library listing
│   │   ├── pages/prompt_detail.rs  # Prompt Builder (editor/composer)
│   │   └── components/
│   │       ├── conversation_list.rs  # Conversation sidebar (Phase 5)
│   │       ├── message_stream.rs     # Streaming event display (Phase 5)
│   │       ├── chat_input.rs         # Manual message input (Phase 5)
│   │       ├── library_dropdown.rs   # Navbar "Library" dropdown → Prompts
│   ├── orchestration/   # State machine, guards, recovery, completion
│   ├── providers/       # LlmProvider trait, opencode_sdk providers
│   │   ├── opencode_sdk_provider.rs  # Pooled opencode server provider
│   │   ├── rig_config.rs             # Rig provider config domain types (capture only)
│   │   └── registry.rs               # Harness dispatch ("opencode", rig guard)
│   ├── agents/          # Prompt builders (planning, impl, review, PR)
│   ├── prompts/         # Prompt render engine (token allowlist, validate, render)
│   ├── services/        # Auth, projects, tasks, settings, session, transcript, export_import, commits, groups, access, prompts
│   ├── archive/         # Task doc I/O, context prompt
│   ├── worktree/        # Git worktree management
│   └── rauthy/          # Local rauthy lifecycle (docker), PUB_URL/env build, endpoint re-hosting helpers
├── tests/               # 13 integration test files
├── templates/           # Agent prompt templates
└── assets/              # Bulma CSS, logos
```

The workspace has a single member crate (`ofm` binary) defined inline.

## Database

- **Engine**: [hiqlite](https://crates.io/crates/hiqlite) — async, Raft-capable embedded SQLite with built-in durability via WAL + auto-heal crash recovery. Single-node deployment eliminates the Mutex bottleneck in axum handlers.
- **Schema**: 15+ tables defined via raw SQL DDL in `src/db/mod.rs`. Project and task IDs use `INTEGER PRIMARY KEY AUTOINCREMENT`; other UUIDs (users, sessions, conversations) are stored as `TEXT`. Booleans are `INTEGER` (0/1), JSON as `TEXT`, and timestamps as naive **UTC** `TEXT` strings in `"YYYY-MM-DD HH:MM:SS"` (no timezone marker).
- **Timestamps contract**: All timestamps are stored as UTC. They are emitted on the wire as naive UTC — space-separated in WS payloads (`"YYYY-MM-DD HH:MM:SS"`) and `T`-separated in JSON via chrono serde (`"YYYY-MM-DDTHH:MM:SS"`). **All display conversion to the browser's local timezone/locale happens client-side** via the `OfmTime` helper in `src/webapp/shim/runtime.rs`: SSR components emit a machine-readable `data-utc="<RFC3339 UTC>"` attribute plus `data-utc-format` (`"pill"` | `"datetime"` | `"date"`) on every rendered timestamp, keeping the server-rendered UTC text as a no-JS fallback, and `OfmTime.apply()` rewrites `textContent` in the browser's zone on `DOMContentLoaded` (and after island fetches). The `utc_attr()` helper in `src/webapp/components/datetime.rs` produces the RFC 3339 UTC attribute value.
- **Migration system**: A `_migrations` tracking table records which migrations have been applied. Each migration is a named SQL DDL statement; only unapplied migrations execute on startup.

### Tables

| Table | Purpose |
|-------|---------|
| `users` | User accounts with OIDC auth |
| `projects` | Project definitions (repo paths, monorepo subproject paths, `tags` JSON array for `{{tags}}` substitution) |
| `prompts` | Prompt Library entries (`snippet`/`composite`/`static`, owner, share/static flags, tags, `static_key`) |
| `prompt_children` | Ordered composite composition (`parent_id`, `child_id`, `position`) |
| `prompt_assignments` | Agent-phase designations (`agent_type`, `scope_type`, `project_id`; unique via index on `(agent_type, scope_type, COALESCE(project_id, -1))`) |
| `groups` | User Group / Organization definitions (name, `is_org`, `is_oauth_scope`, owner, creator snapshot) |
| `group_members` | Polymorphic membership (user OR subgroup) with a fixed `level` enum |
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
| gix | 0.86 | Pure-Rust git repository reading (commits, merge-base, trees, blobs) for the commit list & diff view |
| similar | 3 | Line-diff classification (Equal/Delete/Insert) for stacked diffs |
| hyper / hyper-util | 1 / 0.1 | Low-level HTTP client for the `/auth` reverse proxy (`src/server/proxy.rs`) |
| bytes / http-body-util | 1 / 0.1 | Body buffering for the `/auth` reverse proxy |

## Application Lifecycle

1. **Config**: Load `OfmConfig` from YAML file + env var overlay (`OFM_*`).
2. **Logging**: Initialize tracing/logging based on config.
3. **Database**: Start hiqlite node with `data_dir`, run pending migrations.
4. **Rauthy**: If `OFM_RAUTHY_ENABLED`, spawn rauthy as a Docker container via `tokio::process::Command`, wait for health at the container's direct loopback port, and assign the instance to `_rauthy_instance` before any fallible step so `Drop` removes the container (`docker rm -f ofm-rauthy-<footprint-hash>`) on every failure path. The container binds loopback only (`-p 127.0.0.1:{port}:8080`; the browser reaches rauthy exclusively through OFM's `/auth` proxy, so the published port must not be reachable from the network) and advertises `PUB_URL={host[:port]}` derived from OFM's `pub_url` (`OFM_PUB_URL`). The browser reaches rauthy **exclusively through OFM's `/auth` reverse proxy** (`src/server/proxy.rs`): the axum router nests `/auth` → a hyper-util legacy client forwarding to `http://127.0.0.1:{rauthy_port}/auth/*`, preserving `Host`, overwriting `X-Forwarded-Host`/`X-Forwarded-Proto` from the configured `pub_url` (client-supplied values are never trusted — rauthy in `proxy_mode` trusts its proxy), and overwriting `X-Forwarded-For` with the direct peer IP (an incoming chain is preserved and the peer appended only when the peer is itself a proxy listed in `OFM_RAUTHY_TRUSTED_PROXIES`); the RFC 7239 `Forwarded` header is stripped so a client cannot spoof the source IP rauthy sees. `OFM_RAUTHY_PROXY_MODE`/`OFM_RAUTHY_TRUSTED_PROXIES` pass through to rauthy's `PROXY_MODE`/`TRUSTED_PROXIES` (default off). The `OidcEndpoints` handed to the browser are built by **re-hosting** rauthy's discovery paths onto `pub_url` (`rehost_endpoint` in `src/rauthy/mod.rs`) — rauthy's default mode advertises `http://{pub_url}/auth/v1/...`, so re-hosting fixes the scheme and guarantees a mis-set rauthy `PUB_URL` (e.g. a leftover `127.0.0.1`) can never leak into the authorization URL; OFM's own token/userinfo/revocation/JWKS calls go direct at loopback (the `AuthLayer` uses a `jwks_refresh_url` to refresh keys directly). The bootstrap `ofm` client's `allowed_origins` is set to the configured `pub_url` origin (`client_allowed_origin`) so rauthy's login-form `Origin` check accepts the browser even when its own `pub_url_with_scheme` derivation (http with `proxy_mode` off) differs from `OFM_PUB_URL`'s scheme. The container runs with the host user's UID via Docker's `--user` flag so files in the rauthy data directory are owned by the host user and cleanup does not require root. The footprint-derived container name is stable, so a stale container from a SIGKILLed instance is reaped by the startup `docker rm -f` on the next run of the same footprint. A `pub_url` change on an existing footprint likewise re-bootstraps the rauthy data volume: rauthy imports `clients.json` only on first init, so `ofm` records the bootstrapped `pub_url` in `{footprint}/rauthy/pub_url` and deletes `{footprint}/rauthy/data` when it changes (`ensure_pub_url_bootstrap` in `src/rauthy/mod.rs`), re-creating the admin account at startup. The re-created admin identity has a fresh OIDC `sub`; OFM records the invalidated subjects at `{footprint}/rauthy/relink_subjects`, and on the next login `find_or_create_user` (`src/services/auth.rs`) re-links the existing `users` row by `username` only when its current `oidc_subject` is one of the recorded ones (remapping it to the new subject), so login does not fail on the username UNIQUE constraint — re-link is never granted on a username collision alone.
5. **Server**: Start axum HTTP server with WebSocket support on configured `OFM_HOSTNAME:OFM_PORT`, served via `into_make_service_with_connect_info::<SocketAddr>()` so the `/auth` reverse proxy (`src/server/proxy.rs`) can set `X-Forwarded-For` from the real peer IP.
6. **WebSocket**: Accept connections, manage task subscriptions, stream agent events.
7. **OpenCode provider sessions**: Spawn `opencode serve` subprocesses per user in their own process groups, manage lifecycle, stream events. Teardown kills each spawned process group precisely (`kill(-pgid)`); the pool is drained on shutdown.
8. **Shutdown**: Graceful shutdown — stop accepting connections, kill subprocesses, remove the rauthy container by name, close DB.

## Session Cookie Clearing on Invalidated Refresh

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

The `build_context_prompt()` in `src/archive/mod.rs` builds the agent's turn context
prompt. The historical "OFM Environment" section (which documented a
`.ofm_agent.json` file and `jq` instructions for calling the removed `agent-flags`
endpoints) has been **removed** along with the file and endpoints themselves.
The prompt retains the **Working Directory** (authoritative CWD + allowed paths),
**Task Plan File**, **AGENTS.md Guidance**, and **Testing Configuration** sections.

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

## Shared Agent Run Starting

The `start_next_agent()` function in `src/orchestration/mod.rs` consolidates all agent-run startup logic (config resolution, guard checks, session creation, provider startup, context-prompt building, turn initiation, and broadcast task spawning). Both the HTTP handler (`post_create_agent_run` in `src/server/routes/agent_runs.rs`) and auto-advancement callers use this shared function.

## Auto-Advancement Wiring

When an agent run completes and `completion_handler` returns `NextAction::StartAgent`, the broadcast task (in both `start_next_agent()` and the conversations broadcast task in `send_message()`) spawns a new task that calls `start_next_agent()` for the next phase. This wires auto-advancement through the implementation → review → refinement → PR pipeline with no completion endpoints or scripts.

For a **review** run, `completion_handler` reads the review conversation's **last model message** via `transcript::last_model_text` and does a case-sensitive substring search for the literal keyword `READY`:
- **`READY` present** → finish pipeline: refinement (if configured), else PR (if configured), else Terminal.
- **`READY` absent** → bounce back to implementation (the loop continues).

## Git Commit List & Diff View

The task detail experience exposes the task worktree's git history and per-commit
diffs entirely server-side (no client-side JS polling; every GET re-renders).

- **Service layer** (`src/services/commits.rs`): pure, synchronous gix reads —
  `list_commits_for_worktree` (commits on the worktree branch since the
  merge-base with the base branch, oldest→newest), `commit_diff` (first-parent
  tree diff; empty tree for root commits), `parse_oid` / `resolve_oid`. The base
  branch is resolved at render time mirroring `worktree::detect_default_branch`
  (`refs/remotes/origin/HEAD` → checked-out branch → `"main"`), so the list
  reflects base-branch advances without a schema change. All functions are
  blocking and are invoked from handlers inside `tokio::task::spawn_blocking`
  per AGENTS.md. Any error degrades to an empty state, never a 500.
- **Commit table** (`src/webapp/components/commit_list.rs`): a Bulma `.box`
  titled "Commits" with a `.table` (OID / Message / Author / Date / Files),
  rows oldest→newest, each linking to `/webapp/projects/{project_id}/tasks/{task_id}/commits/{short_oid}`.
  Rendered by `TaskDetailPage` beneath the Documentation box; empty branches
  render "No commits yet.".
- **Per-commit page** (`src/webapp/pages/commit_detail.rs`, route
  `/webapp/projects/{project_id}/tasks/{task_id}/commits/{oid}` in
  `src/webapp/mod.rs`): header box (short OID, summary, author, email,
  timestamp) plus the stacked diff. Bad/unresolvable OIDs render
  "Commit not found." with a back link.
- **DiffView** (`src/webapp/components/diff_view.rs`): renders each changed file
  as a header (path, status, +adds/−dels) plus a single-column stacked table.
  Each pre-aligned `DiffLine` from `similar::TextDiff` becomes one full-width
  row, in `git diff` order: context lines once with a light `diff-ctx`
  background, removed `Delete` lines red immediately above the `Insert` lines
  that replace them. A line-number gutter shows the side-appropriate single
  number (old on context/removed, new on added); `.diff-add`/`.diff-del` tint
  added/deleted rows.
- **Handlers**: `task_detail_handler` fetches the commit list via
  `spawn_blocking` (`.ok().flatten()` → empty vec); `commit_detail_handler`
  resolves the OID (`resolve_oid`) then diffs it. Both live in
  `src/webapp/mod.rs`.

## WebSocket Real-Time Bus

The server maintains a WebSocket hub for live UI updates. Clients subscribe to per-task channels. Events (streaming deltas, agent-run status changes, task-blocked signals) are broadcast to subscribers in real time. Subscription management handles reconnection and scoped interest sets (only the tasks currently visible on screen).

### System topic & global agent status

Beyond the per-task `WsTopic`, a single **System** topic (`WsTopic { kind: System, id: 0 }`) carries global, user-agnostic agent-lifecycle signals. Every agent state transition publishes an `agent_status` event onto it through `orchestration::broadcast_agent_status` (`src/orchestration/mod.rs`):

| Transition | Action | Where |
|---|---|---|
| Agent starting | `refresh` | `start_next_agent` |
| Agent completed | `completed` | `completion_handler` |
| Agent failed (unexpected session end) | `failed` | broadcast task cleanup (orchestration + `conversations.rs`) |
| Agent start failure | `failed` | `start_next_agent` `start_turn` error path |
| Stop Agent | `stopped` | `reset_agent_runs` (`agent_runs.rs`) |
| Run re-activated by a chat message | `refresh` | `send_message` (`conversations.rs`) |
| Question asked (agent paused) | `question` | broadcast event loop |
| Run blocked (missing config) | `blocked` | `start_next_agent` |

The navbar **AgentDropdown** (`src/webapp/components/agent_dropdown.rs`) subscribes to this topic on every page; on any `agent_status` event it re-fetches `GET /api/tasks/agent-status` and re-renders the button (agent count, pulse) and the menu (running agents, open questions, blocked tasks). A 30s poll re-syncs as a safety net in case a frame is dropped.

## Real-Time Chat

The chat view (`/webapp/projects/{project_id}/tasks/{task_id}/chat`) provides a real-time conversation interface:

- **Event broadcasting**: When `post_create_agent_run` starts an agent turn, it calls `provider.start_turn(input)` which returns an `mpsc::Receiver<ProviderEvent>`. A background task reads events, persists them via `transcript::persist_event()`, maps `ProviderEvent` → `ServerMessage::Event`, and broadcasts via `ws_bus` under the task's `WsTopic`. On `Done`, it calls `completion_handler` to advance the state machine.
- **Tool event merging (server-side)** (Task 67): When a `ToolUse` with `result: Some(...)` arrives in the broadcast loop, instead of inserting a new row, the loop calls `transcript::update_tool_event()` to update the existing ToolUse row in-place with the completed input and output. The broadcast uses the `tool_updated` WS event type. This ensures the DB reflects the complete tool snapshot and page reloads show unified tool cards.
- **Provider-agnostic**: The broadcast task consumes `mpsc::Receiver<ProviderEvent>`, staying completely trait-agnostic. `OpenCodeSdkProvider` is the sole built-in provider.
- **Manual chat**: `POST /api/tasks/{task_id}/conversations/{id}/messages` — persists the user message and broadcasts `user_text` exactly once, then delegates to the `resume_or_recreate` helper. The helper loads the transcript, resumes (or, on failure, recreates) the provider turn, and spawns the broadcast task for the response events. It never re-persists or re-broadcasts the user message, which previously produced duplicate blue `.message-user` elements during live streaming (Task 81).
- **Orchestrator phase skip**: When a phase's agent config is missing (no model configured), `post_create_agent_run` creates a `Blocked` run and returns immediately. `next_agent()` checks config statuses before returning `StartAgent`, skipping unconfigured phases.
- **Chat API**: `GET /api/tasks/{task_id}/conversations` lists conversations with their associated runs; `GET /api/tasks/{task_id}/conversations/{id}` returns a conversation with its full message transcript.
- **UI Components**: The chat page has four Leptos SSR components — `ConversationList` (sidebar), `MessageStream` (event display with `overflow-wrap: break-word` bounding), `ChatStatusBar` (permanently-visible, navbar-like status bar above the input showing the agent-type icon/label + model line on the left and the status text + Stop Agent button on the right), and `ChatInput` (message input — agent-type phases dropdown removed in Task 204). The `AgentRunBanner` was removed in Task 2 (notification bar replaced with task detail page's Agents box controls). The old black, transient `#agent-thinking-bar` box was removed in Task 168 in favor of the always-visible `ChatStatusBar`.

## WebApp (Leptos Islands)

All webapp UI follows the Islands Architecture pattern:
- The shell page is SSR-rendered via `leptos::ssr::render_to_string` from a plain axum handler.
- Each functional UI unit ("island") is a Leptos `#[component]` rendered to an HTML fragment by its own axum endpoint (`/webapp/islands/{name}`).
- A minimal inline JS runtime fetches islands and supports re-fetch via `[data-island-refresh]` buttons.
- The same runtime script (`global_runtime_script` in `src/webapp/shim/runtime.rs`, loaded in `<head>` by `ShellPage`) exposes the `OfmTime` helper, which parses naive-UTC timestamp strings as UTC and formats them in the browser's locale/timezone. SSR components emit `data-utc`/`data-utc-format` attributes (see the *Database → Timestamps contract* note); `OfmTime.apply()` localizes them on `DOMContentLoaded` and after island fetches, so the client-side live-update paths (`chat.rs`, `task_detail.rs`) reuse the same formatting as page-load rendering.
- Auth is enforced by the existing `AuthLayer` and per-handler `AuthUser` extractor.
- Styling via Bulma CSS with MDI icons.

### UI Components

- **Breadcrumbs**: Shared breadcrumb navigation system. `BreadcrumbItem` data struct holds `title`, `icon`, and `path`. A `breadcrumb_registry` module centralizes canonical breadcrumb definitions (e.g., `all_projects()`, `project()`, `task()`, `chat()`, `commit()`, `settings()`). Settings pages additionally use `settings_section()` / `settings_sub_page()` to trail breadcrumbs down to the active section and sub-page (e.g. All Projects → Settings → Providers & Agents → Model Configurations). The `Breadcrumbs` Leptos component renders Bulma `<nav class="breadcrumb">` markup. Breadcrumbs flow from page handler -> `render_shell()` -> `ShellPage` -> `Navbar`, appearing immediately after the WS status indicator in the navbar-start div.
- **CommitList** (`src/webapp/components/commit_list.rs`): commit-table `.box` for the task detail page (see *Git Commit List & Diff View*).
- **DiffView** (`src/webapp/components/diff_view.rs`): single-column stacked diff renderer for the commit detail page (see *Git Commit List & Diff View*).
- **SettingsDropdown** (`src/webapp/components/settings_dropdown.rs`): navbar split-button with both the label and arrow buttons toggling a one-level menu listing Providers & Agents, Import/Export, Account (the label no longer navigates directly). Replaced the former separate User Config and Settings navbar buttons.
- **AgentDropdown** (`src/webapp/components/agent_dropdown.rs`): navbar dropdown showing the running-agent count, the live WebSocket connection status, and per-agent entries that link into their chat. Driven exclusively by server activity — it subscribes to the WebSocket **System** topic and re-fetches `GET /api/tasks/agent-status` on every `agent_status` broadcast (see *System topic & global agent status*). The menu also lists open-question tasks ("Needs your input", from `question_asked` events) and blocked tasks. Styling: a 15s pulse on the message-outline icon while ≥ 1 agent runs; cyan when ≥ 1 question task; `is-primary` when ≥ 1 blocked task (trumping all other rules).
- **SettingsSidebar** (`src/webapp/components/settings_sidebar.rs`): section-local Bulma `.menu` sidebar. Defines the `SettingsSection`/`SettingsSubPage` enums and renders exactly one `is-active` link matching the active sub-page.
- **Settings pages** (`src/webapp/pages/settings/`): freestanding pages under `/webapp/settings/*`, each a sidebar + content pane. `providers_agents.rs` (Model Configurations landing + Agent Settings), `rig_providers.rs` (Rig-based Providers — a **capture-only** surface for per-vendor Rig provider configs under "Providers & Agents"; no execution), `import_export.rs` (Export landing + Import), `account.rs` (User Config landing, reuses `OnboardingForm`, + API Keys). `/webapp/settings` is kept as an alias for the Providers & Agents landing. The old tab-switching JS in `pages/settings.rs` was split into per-sub-page scripts (each self-contained, rendered only with its pane).

## Agent Prompt Pipeline

Agent prompts are assembled from templates in `templates/`:
- `templates/planification.md` — planning prompt
- `templates/plan-template.md` — plan output format
- More prompt templates in `src/agents/` for implementation, review, and PR agents.

Prompt assembly functions in `src/agents/planning.rs` and related modules build the turn input from task context, model settings, and template rendering.

### Prompt Library resolution precedes template fallback

`start_next_agent` (`src/orchestration/mod.rs`) first resolves a designated
prompt for the current agent phase via
`services::prompts::resolve_prompt_for_agent` (project-scoped designation wins,
then global). When a designation exists, the prompt's content is recursively
flattened (`services::prompts::flattened_content`, joining composite children
with a `---` separator) and rendered through the `src/prompts/` engine, which
substitutes the 9 standard tokens (`{{taskId}}`, `{{projectId}}`,
`{{taskDocPath}}`, `{{taskWorktreePath}}`, `{{taskWorktreeBranch}}`,
`{{projectDefaultBranch}}`, `{{projectName}}`, `{{taskName}}`, `{{tags}}` —
where `{{tags}}` is the project's `tags` list). On any resolution or render
failure the pipeline falls back to the stock `agents::*::build_*_prompt()`
builders unchanged, so the default flow is untouched unless a user deliberately
designates a prompt. User-authored snippet/composite content is validated
against the token allowlist and dash-based-name tag grammar at create/update
time (non-destructive); static templates are exempt because they use extra,
non-standard tokens. The 6 `templates/*.md` files are seeded as immutable
`kind='static'` rows by `db::ensure_static_prompts` at startup (visible to all,
duplicable into a user's own snippet).

## YAML Config Overlay

Configuration is loaded from YAML files with environment variable overlay:
- Base config from `{footprint}/config/ofm.yml`
- Env vars with `OFM_` prefix override YAML values
- `OFM_FOOTPRINT` (default `~/.ofm`) derives all data paths (DB, archive, config, rauthy)
- `OFM_DB_PATH`, `OFM_ARCHIVE_ROOT`, `OFM_CONFIG` are eliminated in favor of footprint-derived paths

## Groups & Access Control

User Groups / Organizations are the unit of access control for user-owned
resources. The `groups` and `group_members` tables are managed by
`src/services/groups.rs` (create/update/delete groups, add/remove/change-level
members, and recursive membership resolution with a cycle guard) and exposed to
admins via `src/server/routes/groups.rs` (`/api/groups`, admin-only, plus
`GET /api/groups/scopes-available` for the scope dropdown). A bootstrap-seeded
`admins` group is created once per footprint by `db::ensure_admins_group`
(`src/db/mod.rs`), owned by the current ofm admin with an `admin`-level
membership.

Authorization composes ownership + group membership in `src/services/access.rs`:
`has_resource_access(requester, owner_id, min_level)` grants access when the
requester is the owner, a server admin, or an effective member (at `min_level`
or higher) of any group the owner belongs to — resource → group linkage is
modeled as the owner's group membership, so no per-resource `group_id` columns
are needed. It is wrapped per resource (`has_project_access`,
`has_model_config_access`, `has_task_flow_access`, plus write variants) and
threaded through the projects/settings/agent_configs/tasks/conversations/
agent_runs routes, the webapp page handlers, and the agent-status feed
(`get_running_agents` / `get_open_question_tasks` / `get_blocked_tasks`).

OAuth scopes support membership-by-scope: `src/main.rs` parses the discovery
document's optional `scopes_supported` into `OidcEndpoints.scopes_supported`,
`handle_callback` (`src/services/auth.rs`) captures the granted `scope` claim
from the access token/userinfo and unions it with `scopes_supported` onto the
new `users.scopes` column, and `groups::is_member` folds in users whose scopes
match a group flagged `is_oauth_scope`.

## Recurring Patterns

- **snake_case** naming for all columns and Rust identifiers
- **Custom error types** via `src/server/error.rs` — `AppError` enum with typed HTTP responses, replacing `Box<dyn Error>`
- **`TEXT` storage** for UUIDs (users, sessions, conversations, etc.), timestamps (naive UTC `"YYYY-MM-DD HH:MM:SS"`), and JSON values; project/task IDs use `INTEGER` (SQLite convention)
- **`AuthLayer` Tower middleware** for request authentication (JWT via JWKS, API key hash lookup)
- **`spawn_blocking`** for blocking I/O operations (PTY reads), sending events through `mpsc::Sender::blocking_send`
- **Drop non-`Send` rows before awaits**: hiqlite `Row`s are not `Send`, so any query that feeds an access-check `.await` must be extracted to owned values first (see the two-pass loops in `src/services/tasks.rs`).

## Design Decisions

- **hiqlite over rusqlite**: hiqlite provides an async, Raft-capable SQLite database with built-in durability via WAL + auto-heal crash recovery. Single-node deployment eliminates the Mutex bottleneck in axum handlers.
- **OIDC over password auth**: Production-ready authentication without implementing bespoke password handling. Supports enterprise SSO.
- **Embedded DB over client-server**: Eliminates external database infrastructure for development and small-scale deployments. hiqlite manages state files inside the configured `data_dir`.
- **Raw SQL DDL over migration framework**: DDL is wrapped in a simple `_migrations` tracking table, keeping the migration system self-contained.
- **WebSocket for live UI**: Real-time updates via WebSocket subscriptions instead of polling, enabling live agent-streaming and board state updates.
- **Leptos Islands over SPA**: Server-side rendered islands reduce client JS bundle and simplify auth (SSR handlers share server-side auth context without a separate token refresh for the SPA shell).
- **Single harness**: `opencode` is the built-in provider behind the `LlmProvider` trait abstraction, backed by the `opencode_sdk` submodule. A second harness value, `"rig"` (`harness = "rig"` on `user_model_configs` and `agent_harness_configs`), marks configs captured by the **Rig-based Providers** settings surface (`/webapp/settings/providers-agents/rig-providers`, sidebar entry `SettingsSubPage::RigProviders`). Rig configs live as typed `RigProviderConfig` JSON files (`src/providers/rig_config.rs`) under `{config_root}/provider-configs/{uuid}.rig.json`; they are **capture-only** — `registry::resolve_provider` / `resolve_provider_for_user` refuse to resolve `"rig"` with a clear "not yet executable" guard until a future story (RIG 1) adds the Rig execution client.
- **Footprint-derived paths**: `OFM_FOOTPRINT` is the single root for all data directories, eliminating the env-var explosion of `OFM_DB_PATH`, `OFM_ARCHIVE_ROOT`, `OFM_CONFIG`.
