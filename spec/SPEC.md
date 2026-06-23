# Omprint — Specification

[`omprint`][1] orchestrates a small team of
coding agents that collaborate on a single task. You describe the work in a
markdown file; a chain of agents plans it, implements it, reviews it, and
opens a pull request — iterating on their own until the work is done or they
hit something only you can resolve.

This repository is **spec-first**. The specification *is* the product. A
complete, working implementation will be created in the enclosing repository, as
the `omprint` application. A typescript `reference/` application is kept in
this directory, for reference during the implementation of [`omprint`][1] (which
will itself be bootstrapped in [vdaubry/bottega][0] until [omprint][1] is featureful
enough to take over). It is an explicit goal to replace **ALL** citations into
`spec/reference`/`reference/` with citations into the local `omprint` codebase

## `omprint` rust implementation of the `bottega` spec

We are shipping a single rust binary that uses this spec. The original bottega
`reference/` implementation (a typescript codebase) is provided for reference
(with plans to remove as soon as `omprint` is mature).

These files (`SPEC.md`, `core/` and `extra/`) have been modified to suit the desired
scope of `omprint`, and they differ from `bottega` in many respects.

All new code in the `omprint` workspace will be in the Rust programming language.

`omprint` is a single binary application that:

- Serves a web application implementing the client experience in this spec
- Owns one or more `oh-my-pi`/`omp` subprocesses, whose input/output and lifecycle
it drives via RPC over `STDIO`
- A system for driving the `omp` subprocesses, and integrating their input/output
into the `omprint` state
- Hosts an embedded database ([`hiqlite`][3]) with built-in [High availability][8] features

Key `omprint` stack/architectural choices:

- `tokio` + `axum`
  - the core web server
  - OAuth token verification happens with `jsonwebtoken` in a Tower middleware
  - Anchor-point for the `omprint` `leptos` web application
  - Hosts API endpoints called by the `omprint` web application
  - spawns background workers through `tokio` to own *PTY* sessions
- `rustls` + `aws-lc-rs` [crate features][9] are used wherever relavant (tools
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

## Details on the `omprint` server implementation

- On startup, `omprint` will begin listening on the configured `HOSTNAME` +
`PORT`
- Requests to `/` or `/webapp` are for the `omprint` web application
  - specifically: requests to `/` will redirect to `/webapp`
  - all web routes, assets/content, etc lives under `/webapp`
- Requests against `/api` are for the `omprint` `axum` backend server,
which responds to user requests, oversees filesystem actions,
spawns `pty`s, maintains database state, and so on
- If configured to host a `rauthy` instance for OAuth, `omprint` will:
  - Use a `pty` to start an instance of `rauthy`, at a random port
  that differs from the configured `omprint` `PORT`
  - Expose an [axum-based reverse proxy][12] that forwards requests
  and responses to/from `rauthy`; this reverse proxy is exposed
  at `/auth`

## How to build from this spec

**FIXME: replace all occurances of `reference/` with links into the `omprint`
rust codebase**

Point a coding agent at this file and say "build this." Then:

1. Read this file top to bottom.
2. Implement everything in [`core/`](./core). That is the whole product at its
   smallest. The core docs are written as **behavior** — what the tool does and
   why — with technical guidance and pointers into `reference/` for the parts
   that were genuinely hard to get right.
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

`omprint` is meant to stay small. The core is a tight orchestration engine and
nothing more. If your team needs something different — a different coding agent,
another agent role, a different task source — you **fork the behavior into your
own extra**; you don't grow the core.

This is a deliberate stance, and it shapes the spec:

- **Core is universal.** Every `omprint` deployment has it.
- **Extra is preference.** Pick a subset; ignore the rest.
  - `omprint` implements the *entire* surface of `extra/`, undesired
  modules from `bottega` have been removed, and new ones added
- We would rather you build your own extra than ask the core to absorb your
  workflow.

`omprint` is bound to a single coding agent — [`oh-my-pi`/`omp`][2]. It does
**not** abstract over multiple agent backends; the integration is direct and
lives in [`core/omp-integration.md`](./core/omp-integration.md). Wanting a
different agent means forking, not growing the core.

## Core specifications — `core/`

Implement all of these for a minimal working tool. Read them in this order.

| Reviewd/Updated for `omprint`? | Spec | What it covers |
|---|---|---|
| **✅ Yes** | [`core/orchestration-loop.md`](./core/orchestration-loop.md) | **The engine.** The state machine that drives plan → (implement ⇄ review) → PR: agent runs, chaining, the iteration cap, blocking, and how each step decides the next. Start here. |
| **✅ Yes** |  [`core/task-and-workspace.md`](./core/task-and-workspace.md) | The unit of work: a markdown document plus an isolated git worktree. Lifecycle, and where the doc lives so it survives the PR merge. Deliberately silent on how the doc is authored. |
| **✅ Yes** | [`core/omp-integration.md`](./core/omp-integration.md) | The seam between the engine and the coding agent: how `omprint` spawns `omp` in RPC mode over `STDIO` via `portable-pty`, drives sessions/turns, consumes omp's native event stream, persists the transcript, aborts, and handles credentials — direct integration, no abstraction layer. |
| **🚫 No** | [`core/planning-agent.md`](./core/planning-agent.md) | The agent that turns a prompt + task doc into a structured implementation plan written back into the doc. |
| **🚫 No** | [`core/execution-loop.md`](./core/execution-loop.md) | The implementation agent and the thread-review agent, and how they alternate until the work passes review. |
| **🚫 No** | [`core/pull-request-agent.md`](./core/pull-request-agent.md) | The terminal agent: open the PR, drive CI to green, resolve conflicts, and signal completion. |

## Optional specifications — `extra/`

Opinionated features. Each is independent; implement what you want. (The old
`extra/harnesses/` modules — a shared multi-harness overview plus per-tool
provider integrations — are **gone**: `omprint` integrates `omp` directly in
[`core/omp-integration.md`](./core/omp-integration.md), so there is no harness
layer to make optional.)

| Reviewd/Updated for `omprint`? | Spec | What it adds |
|---|---|---|
| **🚫 No** | [`extra/kanban-board.md`](./extra/kanban-board.md) | The opinionated projects/tasks board and 4-screen UI for authoring tasks. |
| **🚫 No** | [`extra/refinement-agent.md`](./extra/refinement-agent.md) | An extra agent that polishes the work between review and PR. |
| **🚫 No** | [`extra/yolo-mode.md`](./extra/yolo-mode.md) | A single-agent alternative to the multi-step pipeline. |
| **🚫 No** | [`extra/pr-comment-retrigger.md`](./extra/pr-comment-retrigger.md) | Re-run the PR agent automatically when a PR receives review comments (periodic PR polling). |
| **🚫 No** | [`extra/prompt-and-model-customization.md`](./extra/prompt-and-model-customization.md) | Per-agent prompt overrides and per-user model/effort selection. |
| **🚫 No** | [`extra/auth-and-multi-user.md`](./extra/auth-and-multi-user.md) | OAuth-integration, Accounts, API keys, project membership, admin, and role-driven behavior (e.g. auto-advancing past the plan gate for non-technical users). |
| **🚫 No** | [`extra/chat-ux.md`](./extra/chat-ux.md) | Manual-chat conveniences: slash commands, file attachments, voice input, title generation, the context-usage meter. |

## The reference implementation

> **⚠️IMPORTANT ⚠️:** The `reference/` implementation LACKS any content related to
> `oh-my-pi` or `omprint`-specific features; Where it is referenced is
> understood as prior behavior that was retained from [vdaubry/bottega][0].
> It is a standing **FIXME** that all instances of `reference/` be replaced
> with links into the `omprint` codebase

`reference/` is a complete, deployed implementation. Use it to resolve any
ambiguity left by the spec.

- **Stack as built:** TypeScript end to end. React 18 + Vite frontend; Node +
  Express + `ws` backend; SQLite (`better-sqlite3`) for all state, including
  conversation transcripts. You are not required to match this stack — the spec
  describes behavior — but the reference assumes it, so its citations are
  TypeScript.
- **Where to start reading:** [`reference/server/database/init.sql`](./reference/server/database/init.sql)
  (the whole data model in one file) and [`reference/docs/project.md`](./reference/docs/project.md)
  (an architecture tour).
- **Citations:** spec files link to specific files and, where it helps, methods
  or line ranges. Treat each as "here is how we solved it," not "copy this."

## Non-goals

- Supporting any coding harness besides [`oh-my-pi`/`omp`][2]. That is
  what `extra/` and forking are for.
- Backwards-compatibility shims, configuration for hypothetical needs, or
  opt-out flags. Keep the core small.

[0]: https://github.com/vdaubry/bottega
[1]: https://github.com/olsonjeffery/omprint
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
