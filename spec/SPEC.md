# ofm — Specification

> **Webapp UI Architecture (Island Pattern)**: All webapp UI follows Jason Miller's
> Islands Architecture. The shell page is SSR-rendered via `leptos::ssr::render_to_string`
> from a plain axum handler. Each functional UI unit ("island") is a Leptos
> `#[component]` rendered to an HTML fragment by its own axum endpoint
> (`/webapp/islands/{name}`). A minimal inline JS runtime fetches islands and
> supports re-fetch via `[data-island-refresh]` buttons. Auth is enforced by the
> existing `AuthLayer` and per-handler `AuthUser` extractor. Styling via
> `leptos_styling` with `style_sheet!` macro. No WASM, no `leptos_axum`,
> no `leptos_router`. See `src/webapp/`.

> **⚠️`ofm` ONLY ⚠️:** Rust convention requires functions and `let` bindings
> use `snake_case` as a naming convention; In all places where `camelCase`
> occurs (referring to the typescript `reference/` implementation of `bottega`),
> substitute for `snake_case` as appropriate; `PascalCase` is used for `trait`s,
> `struct`s, `enum`s, etc.
> 
> **Note:** The `ofm` Rust codebase at `src/` now provides implementations
> for many of the features described in the spec. Prefer citations to `src/`
> over `reference/` wherever equivalents exist.

[`ofm`][1] orchestrates a small team of
coding agents that collaborate on a single task. You describe the work in a
markdown file; a chain of agents plans it, implements it, reviews it, and
opens a pull request — iterating on their own until the work is done or they
hit something only you can resolve.

This repository is **spec-first**. The specification *is* the product. A
complete, working implementation will be created in the enclosing repository, as
the `ofm` application. A typescript `reference/` application is kept in
this directory, for reference during the implementation of [`ofm`][1]. The
Rust codebase's foundational layer (DB schema, CRUD API, worktree management,
OpenCode provider integration (via `opencode_sdk`), task archive, orchestration state machine,
agent prompt builders) is now implemented at `src/`. The orchestration loop
completion handler, state machine transitions, and agent prompt builders for
planning, implementation, review, and PR are implemented. The full
implementation/review agent loop wiring (chaining through the completion
handler) is partially wired; the reference is retained for the remaining
end-to-end lifecycle details.

## `ofm` rust implementation of the `bottega` spec

We are shipping a single rust binary that uses this spec. The original bottega
`reference/` implementation (a typescript codebase) is provided for reference
(with plans to remove as soon as `ofm` is mature).

These files (`SPEC.md`, `core/` and `extra/`) have been modified to suit the desired
scope of `ofm`, and they differ from `bottega` in many respects.

All new code in the `ofm` workspace will be in the Rust programming language.

`ofm` is a single binary application that:

- Serves a web application implementing the client experience in this spec
- Owns one or more coding-harness subprocesses: OpenCode (via `opencode_sdk`), whose input/output and lifecycle
it drives
- A system for driving the harness subprocesses, and integrating their input/output
into the `ofm` state
- Hosts an embedded database ([`hiqlite`][3]) with built-in [High availability][8] features

Key `ofm` stack/architectural choices:

- `tokio` + `axum`
  - the core web server
  - OAuth token verification happens with `jsonwebtoken` in a Tower middleware
  - Anchor-point for the `ofm` `leptos` web application
  - Hosts API endpoints called by the `ofm` web application
  - spawns background workers through `tokio` to own *PTY* sessions
- `rustls` + `aws-lc-rs` [crate features][9] are used wherever relevant (tools
doing IO requiring SSL); the goal is to completely eschew any system OpenSSL
dependency
- [`leptos`][11], with SSR, as the web application framework
  - Provides the client [OAUTH Authorization Code Flow + PKCE][5]
  - The user onboarding & configuration experience is managed here
  - The core [SDLC loop][10] is mediated through this UX
  - Usages of `git` happen via `pty` sub-processes
  - Spawn instances of the OpenCode coding harness during the loop
- Determine if the built-in git/github support is sufficient to replace the
`bottega` dependency on the `gh` cli tool

## Details on the `ofm` server implementation

- **Data footprint**: All ofm data (database, archive, config, and dependencies'
  data) lives under `OFM_FOOTPRINT` (default `~/.ofm`). Fixed sub-directories:

  | Sub-path | Purpose |
  |---|---|
  | `{footprint}/hiqlite/` | hiqlite embedded database files |
  | `{footprint}/archive/` | Tasks, projects, recordings, and other text files |
  | `{footprint}/config/` | Cookie key, provider configs (`models.yml` etc.) |
  | `{footprint}/rauthy/` | Rauthy persistent state (when self-hosted) |

  The env vars `OFM_DB_PATH`, `OFM_ARCHIVE_ROOT`, and `OFM_CONFIG` are
  eliminated in favor of deriving these paths from `OFM_FOOTPRINT`.

- On startup, `ofm` will begin listening on the configured `OFM_HOSTNAME` +
`OFM_PORT`
- Requests to `/` or `/webapp` are for the `ofm` web application
  - specifically: requests to `/` will redirect to `/webapp`
  - all web routes, assets/content, etc lives under `/webapp`
  - the settings area is split into freestanding pages under `/webapp/settings`:
    `/webapp/settings` (alias → Providers & Agents landing),
    `/webapp/settings/providers-agents/*`, `/webapp/settings/import-export/*`, and
    `/webapp/settings/account/*`. Each 2nd-level route renders a Bulma `.menu`
    sidebar of section sub-pages plus a content pane (see `src/webapp/pages/settings/`
    and `src/webapp/components/settings_sidebar.rs`); the navbar exposes a combined
    "Settings" split-button dropdown (`src/webapp/components/settings_dropdown.rs`)
    replacing the former separate User Config and Settings buttons. Both the label
    and the arrow buttons toggle the one-level menu (the label no longer navigates
    directly). Settings pages show breadcrumbs down to the active section and
    sub-page (e.g. All Projects → Settings → Providers & Agents → Model
    Configurations) via `breadcrumb_registry::settings_section` /
    `settings_sub_page`.
- Requests against `/api` are for the `ofm` `axum` backend server,
which responds to user requests, oversees filesystem actions,
spawns `pty`s, maintains database state, and so on
- If configured to host a `rauthy` instance for OAuth, `ofm` will:
   - Start a Docker container (`ghcr.io/sebadob/rauthy:latest`) at a random port
   that differs from the configured `ofm` `OFM_PORT`, managed via
   `tokio::process::Command` (see `src/rauthy/mod.rs`)
   - Bind the container to loopback (`-p 127.0.0.1:{port}:8080`, so rauthy is
   not reachable from the network — the browser reaches it only through OFM's
   `/auth` proxy) and advertise
   `PUB_URL={host[:port]}` derived from OFM's `pub_url` (`OFM_PUB_URL`, default
   `http://{connectable_host}:{OFM_PORT}`), so rauthy's OIDC discovery metadata
   and referral URLs point at the origin `ofm` is reachable on
   - Proxy all browser traffic to rauthy through OFM's `/auth` route (see
   `src/server/proxy.rs`): the browser hits `{pub_url}/auth/*`, OFM forwards
   verbatim to `http://127.0.0.1:{rauthy_port}/auth/*` while preserving the
   incoming `Host` header, overwriting `X-Forwarded-Host`/`X-Forwarded-Proto`
   from the configured `pub_url` (client-supplied values are never trusted —
   rauthy in `proxy_mode` trusts its proxy) and appending the peer IP to
   `X-Forwarded-For`. This satisfies "bind to one port, accept
   hosts on different ports" (see
   [`extra/auth-and-multi-user.md`](./extra/auth-and-multi-user.md))
   - Build the `OidcEndpoints` handed to the browser off OFM's `pub_url`, **not**
   rauthy's self-reported discovery URLs: rauthy's default mode advertises
   `http://{pub_url}/auth/v1/...` (scheme from `LISTEN_SCHEME`), so OFM re-hosts
   the browser-facing authorization/end-session endpoints onto `pub_url` with
   its configured scheme (`rehost_endpoint` in `src/rauthy/mod.rs`). This keeps
   the authorization URL on the public origin even when the `pub_url` scheme is
   `https`, and makes it impossible for a mis-set rauthy `PUB_URL` (e.g. a
   leftover `127.0.0.1`) to leak into the URL the browser is redirected to. OFM's
   own server-side calls (token exchange, userinfo, revocation, JWKS refresh)
   go direct at loopback, not through the public origin.
   - Optionally run rauthy in "behind proxy" mode via `OFM_RAUTHY_PROXY_MODE` /
   `OFM_RAUTHY_TRUSTED_PROXIES` (rauthy `PROXY_MODE` / `TRUSTED_PROXIES` env).
   **Defaults off.** When enabled, rauthy blocks every request whose source IP is
   not in `TRUSTED_PROXIES` and — per rauthy's source — hardcodes an `https://`
   issuer regardless of `LISTEN_SCHEME`/`X-Forwarded-Proto`; only enable it when
   the `pub_url` origin is served over HTTPS by the enclosing reverse proxy.
   - Bootstrap `clients.json` also lists the configured `pub_url` origin in the
   `ofm` client's `allowed_origins` (`client_allowed_origin` in
   `src/rauthy/mod.rs`): rauthy rejects any login-form/OIDC POST whose `Origin`
   differs from its own `pub_url_with_scheme` (`400 BadRequest: Coming from an
   external Origin`), and that derivation is `http://` with `proxy_mode` off —
   so without this entry an `https://` `pub_url` with `proxy_mode` off would
   make login impossible. Entries that cannot round-trip rauthy's origin
   validation (e.g. IPv6-literal hosts) are omitted rather than failing the
   bootstrap import.
   - Re-bootstrap rauthy when `pub_url` changes on an existing footprint: rauthy
   imports the bootstrap `clients.json` (redirect URIs = `{pub_url}/*`) only on
   first initialization, so a changed public origin would otherwise be rejected
   with `400 Invalid redirect uri` on every login. `ofm` records the bootstrapped
   `pub_url` in `{footprint}/rauthy/pub_url` and deletes the rauthy data volume
   (`{footprint}/rauthy/data`) on change before starting the container
   (`ensure_pub_url_bootstrap` in `src/rauthy/mod.rs`), re-creating the admin
   account at startup. The re-created admin identity has a **fresh OIDC `sub`**;
   `ofm` records the now-invalidated `oidc_subject`s at
   `{footprint}/rauthy/relink_subjects`, and on the next login re-links the
   existing `users` row by `username` only if its current subject is one of
   those recorded (remapping `oidc_subject` to the new subject —
   `find_or_create_user` in `src/services/auth.rs`), so login succeeds instead
   of failing on the username UNIQUE constraint. Re-linking is never authorized
   by a username collision alone (`preferred_username` is a claim-controlled
   value); without a recorded re-bootstrap the login is refused. Previously
   issued rauthy sessions/tokens are invalidated.

### Timestamps

All timestamps are stored as naive **UTC** `TEXT` strings in `"YYYY-MM-DD HH:MM:SS"`
(no timezone marker) and written via `chrono::Utc::now().naive_utc()`. They are
emitted on the wire as naive UTC — space-separated in WebSocket payloads
(`"YYYY-MM-DD HH:MM:SS"`) and `T`-separated in JSON via chrono serde
(`"YYYY-MM-DDTHH:MM:SS"`). **Display conversion to the browser's local
timezone/locale is done entirely on the frontend.** SSR components keep the
server-rendered UTC text as a no-JS fallback and additionally emit a
machine-readable `data-utc="<RFC3339 UTC>"` attribute plus a
`data-utc-format` (`"pill"` | `"datetime"` | `"date"`) hint; the client-side
`OfmTime` helper (`global_runtime_script` in `src/webapp/shim/runtime.rs`,
`utc_attr()` in `src/webapp/components/datetime.rs`) parses the value as UTC
and rewrites the element's text in the browser's zone on `DOMContentLoaded`
and after island fetches. Live WebSocket-update paths (`src/webapp/pages/chat.rs`,
`src/webapp/pages/task_detail.rs`) reuse the same `OfmTime` formatting so
live values always agree with page-load rendering.

## How to build from this spec

Point a coding agent at this file and say "build this." Then:

1. Read this file top to bottom.
2. Implement everything in [`core/`](./core). That is the whole product at its
   smallest. The core docs are written as **behavior** — what the tool does and
   why — with technical guidance and pointers into `reference/` (and increasingly `src/`)
   for the parts that were genuinely hard to get right. Direct Rust implementations
    exist in `src/worktree/mod.rs`
   (worktree create/remove/status), `src/archive/mod.rs` (task doc I/O, archive
   cleanup, context prompt assembly), `src/orchestration/` (state machine,
   completion handler, guards, recovery), `src/providers/` (LlmProvider trait, OpenCodeSdkProvider, config resolution, registry), and
   `src/agents/planning.rs` (planning prompt assembly),
   `src/agents/implementation.rs` (implementation prompt),
   `src/agents/review.rs` (review prompt),
   `src/agents/pull_request.rs` (PR prompt).
    The web application lives at `src/webapp/` (Leptos SSR + islands). The settings
    UI lives at `src/webapp/pages/settings/` (per-section modules `providers_agents.rs`,
    `rig_providers.rs`, `import_export.rs`, `account.rs`) with a shared section-local sidebar in
    `src/webapp/components/settings_sidebar.rs` and a navbar split-button dropdown in
    `src/webapp/components/settings_dropdown.rs`.
    CRUD service logic lives at `src/services/` (auth, projects, tasks, settings, export_import).
   Authentication and OAuth middleware lives at `src/auth/`.
3. Implement whichever [`extra/`](./extra) features you want. These are
   **opinionated**: they reflect one company's preferences, not universal
   truths. Skip any of them and core still works.

`reference/` is a citation, not a copy target. When a spec says "see
`reference/server/services/agentRunner.ts`," open it to learn *how* a problem
was solved, then implement it your way. The spec is the source of truth; where
the two disagree, the spec wins.

## The core value proposition

One thing, done well: **orchestrate multiple agents collaborating on one task
that is defined by a markdown file.**

```
planning ──▶ ( implementation ⇄ review ) ──▶ pull request
```

The tool does not care how the markdown file came to exist. We happen to ship a
Kanban board for authoring tasks, but you might wire tasks to Jira, Notion, or a
plain file in a repo. That is exactly why the board is an *extra*, not core.

## Design philosophy: small and simple

`ofm` is meant to stay small. The core is a tight orchestration engine and
nothing more. If your team needs something different — ~another harness~, another
agent role, a different task source — you **fork the behavior into your own
extra**; you don't grow the core.

This is a deliberate stance, and it shapes the spec:

- **Core is universal.** Every `ofm` deployment has it.
- **Extra is preference.** Pick a subset; ignore the rest.
  - `ofm` implements the *entire* surface of `extra/`, undesired
  modules from `bottega` have been removed, and new ones added
- We would rather you build your own extra than ask the core to absorb your
  workflow.

## Core specifications — `core/`

Implement all of these for a minimal working tool. Read them in this order.

| Reviewed/Updated for `ofm`? | Spec | What it covers |
|---|---|---|
| **✅ Yes** | [`core/orchestration-loop.md`](./core/orchestration-loop.md) | **The engine.** The state machine that drives plan → (implement ⇄ review) → PR: agent runs, chaining, the iteration cap, blocking, and how each step decides the next. Start here. |
| **✅ Yes** |  [`core/task-and-workspace.md`](./core/task-and-workspace.md) | The unit of work: a markdown document plus an isolated git worktree. Lifecycle, and where the doc lives so it survives the PR merge. Deliberately silent on how the doc is authored. |
| **✅ Yes** | (content moved to [`extra/harnesses/opencode.md`](./extra/harnesses/opencode.md)) | The direct OpenCode integration: spawning via `std::process::Command`, the HTTP+SSE protocol, per-turn input, the streaming runtime, transcript persistence, session management, `opencode.json` passthrough, and orphan recovery. See also the provider abstraction at `src/providers/` (`LlmProvider` trait, `OpenCodeSdkProvider`, config resolution). |
| **✅ Yes** | [`core/planning-agent.md`](./core/planning-agent.md) | The agent that turns a prompt + task doc into a structured implementation plan written back into the doc. |
| **⚠️ Partial** | [`core/execution-loop.md`](./core/execution-loop.md) | The implementation agent and the review agent, and how they alternate until the work passes review. Prompt builders exist at `src/agents/implementation.rs` and `src/agents/review.rs`; full turn-lifecycle wiring is pending per `core/execution-loop.md`. |
| **⚠️ Partial** | [`core/pull-request-agent.md`](./core/pull-request-agent.md) | The terminal agent: open the PR, drive CI to green, resolve conflicts, and signal completion. PR prompt builder exists at `src/agents/pull_request.rs`; full PR agent lifecycle (CI monitoring, conflict resolution, merge) is not yet wired. |

## Optional specifications — `extra/`

Opinionated features. Each is independent; implement what you want.

| Reviewed/Updated for `ofm`? | Spec | What it adds |
|---|---|---|
| **✅ Yes** | [`extra/harnesses/opencode.md`](./extra/harnesses/opencode.md) | OpenCode integration: SDK-backed subprocess lifecycle, event mapping, transcript mirroring, credential delegation, and capabilities. |
 | **✅ Yes** | [`extra/harnesses/opencode.md`](./extra/harnesses/opencode.md) | OpenCode SDK-backed provider integration: SDK-driven subprocess lifecycle, event mapping, credential delegation via `opencode.json`, session lifecycle. **Task 204 additions:** `provider_session_id` rename (provider-agnostic), `resume_turn` implementation for `OpenCodeSdkProvider`, `question.asked` event handling (mid-turn question → pause SSE → user reply → resume), `SessionStart` event persistence to DB, lazy provider recreation on restart. |
| **✅ Yes** | [`extra/kanban-board.md`](./extra/kanban-board.md) | The opinionated projects/tasks board and 4-screen UI for authoring tasks. **Task 144 additions:** task detail page now renders a commit-list table (worktree commits since the merge-base with the base branch, oldest→newest, server-side on every load) and each commit links to a dedicated page with the changed-file list and a two-column diff — see [`core/task-and-workspace.md`](./core/task-and-workspace.md) and `src/services/commits.rs`. |
| **🚫 No** | [`extra/refinement-agent.md`](./extra/refinement-agent.md) | An extra agent that polishes the work between review and PR. |
| **🚫 No** | [`extra/yolo-mode.md`](./extra/yolo-mode.md) | A single-agent alternative to the multi-step pipeline. |
| **🚫 No** | [`extra/pr-comment-retrigger.md`](./extra/pr-comment-retrigger.md) | Re-run the PR agent automatically when a PR receives review comments (periodic PR polling). |
| **⚠️ Partial** | [`extra/prompt-and-model-customization.md`](./extra/prompt-and-model-customization.md) | Harness-model config via `agent_harness_configs` and scope-precedence resolution is implemented (`src/providers/`); a **Rig-based Providers** sub-page under "Providers & Agents" (`/webapp/settings/providers-agents/rig-providers`) captures per-vendor Rig provider configs as structured JSON files with `harness = "rig"` (execution deferred to a future story, RIG 1); prompt overrides and template engine are not yet implemented. |
| **✅ Yes** | [`extra/auth-and-multi-user.md`](./extra/auth-and-multi-user.md) | OAuth-integration, Accounts, API keys, admin, and role-driven behavior. Auth infrastructure (AuthLayer, JWKS, PKCE flow, OAuth callback, API keys, rauthy Docker lifecycle) is implemented at `src/auth/`, `src/services/auth.rs`, `src/server/routes/auth.rs`, `src/webapp/auth.rs`, `src/rauthy/`. **User Groups / Organizations** are first-class: `groups` + `group_members` tables, a bootstrap-seeded `admins` group, OAuth scope capture + discovery (`users.scopes`), group-based gating of Projects / Model Configurations / Task Flows (`src/services/groups.rs`, `src/services/access.rs`, `src/server/routes/groups.rs`), and an admin **Groups & Organizations** settings sub-page. The reference's `project_members` join table is folded into groups and dropped; `is_technical` auto-advance remains from the reference. |
| **✅ Yes** | [`extra/chat-ux.md`](./extra/chat-ux.md) | Real-time chat view (`src/webapp/pages/chat.rs`), conversation sidebar (`src/webapp/components/conversation_list.rs`), streaming message display (`src/webapp/components/message_stream.rs`), manual chat input (`src/webapp/components/chat_input.rs`), chat API endpoints (`src/server/routes/conversations.rs`), broadcast task in `post_create_agent_run` (`src/server/routes/agent_runs.rs`), orchestrator phase-skip (`src/orchestration/state_machine.rs`), task detail page Agents box with per-phase Run buttons and Stop Agent button (`src/webapp/pages/task_detail.rs`). **Task 204 additions:** Removed agent-type phases dropdown from `ChatInput`, bounded message timeline with overflow fixes, `question_asked` event rendering in message stream and inline JS. **Task 79 additions:** global navbar agent-status dropdown (`src/webapp/components/agent_dropdown.rs`) driven by System-topic `agent_status` broadcasts — running-agent count, open-question ("Needs your input") and blocked sections, cyan/primary trigger tinting, and a 15s pulse on the message icon; aggregate feed via `GET /api/tasks/agent-status` (`get_global_agent_status` in `src/services/tasks.rs`). Manual conveniences (slash commands, file attachments, voice input, context-usage meter) are not yet implemented. |
| **✅ Yes** | [`extra/opencodeai-sdk.md`](./extra/opencodeid-sdk.md) | API surface and implementation following the example of the `@opencode-ai/sdk` npm package. |

## The reference implementation

> **⚠️ IMPORTANT ⚠️:** The `reference/` implementation LACKS any content related to
> `ofm`-specific features; Where it is referenced is
> understood as prior behavior that was retained from [vdaubry/bottega][0].
> It is a standing **FIXME** that all instances of `reference/` be replaced
> with links into the `ofm` codebase

`reference/` is retained for features not yet ported to Rust. Where a Rust
equivalent exists at `src/`, prefer that citation.

- **Stack as built:** TypeScript end to end (React 18 + Vite frontend; Node +
  Express + `ws` backend; SQLite (`better-sqlite3`) for all state). The
  `ofm` Rust implementation uses `tokio` + `axum` + `hiqlite` instead.
  You are not required to match either stack — the spec describes behavior —
  but the reference assumes TypeScript, so its citations use that language.
- **Where to start reading:** [`reference/server/database/init.sql`](./reference/server/database/init.sql)
  (the whole data model in one file) and [`reference/docs/project.md`](./reference/docs/project.md)
  (an architecture tour).
- **Citations:** spec files link to specific files and, where it helps, methods
  or line ranges. Treat each as "here is how we solved it," not "copy this."
  **Prefer `src/` citations over `reference/` wherever Rust equivalents exist.**

## Session Directory Routing

When `start_next_agent()` in `src/orchestration/mod.rs` creates a new agent session via the opencode SDK,
the provider routes the task worktree path to the opencode server as a `directory` **query param** on every
workspace-scoped HTTP call — `session.create`, `event.subscribe`, `promptAsync`, and `session.abort` —
rather than mutating the shared server's process CWD. In `OpenCodeSdkProvider::start()` the pooled client
handle is re-scoped with `OpencodeClient::with_directory(&worktree)` (an `Arc::make_mut` clone that affects
only this provider's handle), and the client methods in `src/opencode_sdk/client.rs` append the `directory`
query param when set. This mirrors the reference implementation (`spec/reference/server/services/providers/opencode/index.ts`),
whose `WorkspaceRoutingMiddleware` resolves the workspace directory per HTTP call and falls back to the
server's `process.cwd()` when absent — a PATCH to the session row is **not** sufficient because tool calls
re-resolve the directory per request. The `external_directory` allowlist in the server config already
restricts writes to `{footprint}/worktrees/**`, `{footprint}/archive/**`, and `/tmp/**`.

## Auto-Advancement Wiring

When an agent run completes, the `completion_handler` decides the next phase and
the broadcast task calls `start_next_agent()` to automatically start it. This
wires the implementation → review → (refinement →) PR pipeline end-to-end with
no completion endpoints or scripts.

For a **review** run, the handler reads the review conversation's **last model
message** (`transcript::last_model_text`) and does a case-sensitive substring
search for the literal keyword `READY`:

- **`READY` present** → the feature is approved → start refinement (if
  configured), else PR (if configured), else Terminal.
- **`READY` absent** → bounce back to implementation (the loop continues).

The shared `start_next_agent()` function in `src/orchestration/mod.rs` contains
all agent-run startup logic and is used by both the HTTP handler and
auto-advancement callers.

## Non-goals

- Supporting any coding harness beyond the built-in OpenCode provider. That is
  what `extra/` and forking are for.
- Backwards-compatibility shims, configuration for hypothetical needs, or
  opt-out flags. Keep the core small.

[0]: https://github.com/vdaubry/bottega
[1]: https://github.com/olsonjeffery/ofm
[3]: https://github.com/sebadob/hiqlite
[4]: https://github.com/wezterm/wezterm/tree/main/pty
[5]: https://auth0.com/docs/get-started/authentication-and-authorization-flow/authorization-code-flow-with-pkce
[6]: https://en.wikipedia.org/wiki/Pseudoterminal
[7]: https://github.com/sebadob/rauthy
[8]: https://en.wikipedia.org/wiki/High_availability
[9]: https://doc.rust-lang.org/cargo/reference/features.html
[10]: https://en.wikipedia.org/wiki/Systems_development_life_cycle
[11]: https://www.leptos.dev/
