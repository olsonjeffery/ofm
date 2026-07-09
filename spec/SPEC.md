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
OMP subprocess integration, task archive, orchestration state machine,
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
- Owns one or more `oh-my-pi`/`omp` subprocesses, whose input/output and lifecycle
it drives via RPC over `STDIO`
- A system for driving the `omp` subprocesses, and integrating their input/output
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
dependency (**NOTE:** this does not apply to `omp` itself)
- [`leptos`][11], with SSR, as the web application framework
  - Provides the client [OAUTH Authorization Code Flow + PKCE][5]
  - The user onboarding & configuration experience is managed here
  - The core [SDLC loop][10] is mediated through this UX
- [`wezterm`'s `portable-pty` crate][4]
  - A [cross-platform psuedoterminal][6] (aka `pty`), able to spawn
  sub-processes and communicate over `STDIO`
  - Spawn `pty`s to fetch tools during *onboarding*, or as-needed
  - If configured, a `pty` will be created on startup to manage a
  [rauthy][7] instance
  - Usages of `git` happen via `pty` sub-processes
  - Spawn instances of `omp` during the loop and manage their lifecycle
- Determine if `omp`'s git/github support is sufficient to replace the
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
- Requests against `/api` are for the `ofm` `axum` backend server,
which responds to user requests, oversees filesystem actions,
spawns `pty`s, maintains database state, and so on
- If configured to host a `rauthy` instance for OAuth, `ofm` will:
  - Use a `pty` to start an instance of `rauthy`, at a random port
  that differs from the configured `ofm` `OFM_PORT`
  - Expose an [axum-based reverse proxy][12] that forwards requests
  and responses to/from `rauthy`; this reverse proxy is exposed
  at `/auth`

## How to build from this spec

Point a coding agent at this file and say "build this." Then:

1. Read this file top to bottom.
2. Implement everything in [`core/`](./core). That is the whole product at its
   smallest. The core docs are written as **behavior** — what the tool does and
   why — with technical guidance and pointers into `reference/` (and increasingly `src/`)
   for the parts that were genuinely hard to get right. Direct Rust implementations
   exist in `src/omp/mod.rs` (PTY subprocess lifecycle), `src/worktree/mod.rs`
   (worktree create/remove/status), `src/archive/mod.rs` (task doc I/O, archive
   cleanup, context prompt assembly), `src/orchestration/` (state machine,
   completion handler, guards, recovery), `src/providers/` (LlmProvider trait,
   OmpProvider, OpenCodeProvider, config resolution, registry), and
   `src/agents/planning.rs` (planning prompt assembly),
   `src/agents/implementation.rs` (implementation prompt),
   `src/agents/review.rs` (review prompt),
   `src/agents/pull_request.rs` (PR prompt).
   The web application lives at `src/webapp/` (Leptos SSR + islands).
   CRUD service logic lives at `src/services/` (auth, projects, tasks, settings).
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
| **✅ Yes** | [`core/omp-integration.md`](./core/omp-integration.md) | The direct `omp` integration: spawning via `portable-pty`, the RPC message protocol, per-turn input, the streaming runtime, transcript persistence, session management, `models.yml` passthrough, and orphan recovery. See also the provider abstraction at `src/providers/` (`LlmProvider` trait, `OmpProvider`, `OpenCodeProvider`, config resolution). |
| **✅ Yes** | [`core/planning-agent.md`](./core/planning-agent.md) | The agent that turns a prompt + task doc into a structured implementation plan written back into the doc. |
| **⚠️ Partial** | [`core/execution-loop.md`](./core/execution-loop.md) | The implementation agent and the review agent, and how they alternate until the work passes review. Prompt builders exist at `src/agents/implementation.rs` and `src/agents/review.rs`; full turn-lifecycle wiring is pending per `core/execution-loop.md`. |
| **⚠️ Partial** | [`core/pull-request-agent.md`](./core/pull-request-agent.md) | The terminal agent: open the PR, drive CI to green, resolve conflicts, and signal completion. PR prompt builder exists at `src/agents/pull_request.rs`; full PR agent lifecycle (CI monitoring, conflict resolution, merge) is not yet wired. |

## Optional specifications — `extra/`

Opinionated features. Each is independent; implement what you want.

| Reviewed/Updated for `ofm`? | Spec | What it adds |
|---|---|---|
| **✅ Yes** | [`extra/harnesses/omp.md`](./extra/harnesses/omp.md) | `oh-my-pi`/`omp` integration: subprocess lifecycle, event mapping, transcript mirroring, credential delegation, and capabilities. |
| **✅ Yes** | [`extra/harnesses/opencode.md`](./extra/harnesses/opencode.md) | OpenCode integration: HTTP+SSE subprocess lifecycle, event mapping, credential delegation via `opencode.json`, session lifecycle. |
| **✅ Yes** | [`extra/kanban-board.md`](./extra/kanban-board.md) | The opinionated projects/tasks board and 4-screen UI for authoring tasks. |
| **🚫 No** | [`extra/refinement-agent.md`](./extra/refinement-agent.md) | An extra agent that polishes the work between review and PR. |
| **🚫 No** | [`extra/yolo-mode.md`](./extra/yolo-mode.md) | A single-agent alternative to the multi-step pipeline. |
| **🚫 No** | [`extra/pr-comment-retrigger.md`](./extra/pr-comment-retrigger.md) | Re-run the PR agent automatically when a PR receives review comments (periodic PR polling). |
| **⚠️ Partial** | [`extra/prompt-and-model-customization.md`](./extra/prompt-and-model-customization.md) | Harness-model config via `agent_harness_configs` and scope-precedence resolution is implemented (`src/providers/`); prompt overrides and template engine are not yet implemented. |
| **✅ Yes** | [`extra/auth-and-multi-user.md`](./extra/auth-and-multi-user.md) | OAuth-integration, Accounts, API keys, project membership, admin, and role-driven behavior. Note: only `ensure_default_user` is implemented in the Rust codebase. |
| **✅ Yes** | [`extra/chat-ux.md`](./extra/chat-ux.md) | Manual-chat conveniences: slash commands, file attachments, voice input, title generation (implemented in `src/providers/mod.rs`), the context-usage meter. |

## The reference implementation

> **⚠️ IMPORTANT ⚠️:** The `reference/` implementation LACKS any content related to
> `oh-my-pi` or `ofm`-specific features; Where it is referenced is
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

## Non-goals

- Supporting any coding harness besides [`oh-my-pi`/`omp`][2]. That is
  what `extra/` and forking are for.
- Backwards-compatibility shims, configuration for hypothetical needs, or
  opt-out flags. Keep the core small.

[0]: https://github.com/vdaubry/bottega
[1]: https://github.com/olsonjeffery/ofm
[2]: https://omp.sh/
[3]: https://github.com/sebadob/hiqlite
[4]: https://github.com/wezterm/wezterm/tree/main/pty
[5]: https://auth0.com/docs/get-started/authentication-and-authorization-flow/authorization-code-flow-with-pkce
[6]: https://en.wikipedia.org/wiki/Pseudoterminal
[7]: https://github.com/sebadob/rauthy
[8]: https://en.wikipedia.org/wiki/High_availability
[9]: https://doc.rust-lang.org/cargo/reference/features.html
[10]: https://en.wikipedia.org/wiki/Systems_development_life_cycle
[11]: https://www.leptos.dev/
[12]: https://github.com/tokio-rs/axum/blob/main/examples/reverse-proxy/src/main.rs
