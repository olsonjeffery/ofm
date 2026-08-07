# Core — The orchestration loop

> **⚠️`ofm` ONLY ⚠️:** Rust convention requires functions and `let` bindings
> use `snake_case` as a naming convention. In all places where `camelCase`
> occurs (in citations from the legacy typescript `reference/` implementation),
> substitute for `snake_case` as appropriate; `PascalCase` is used for `trait`s,
> `struct`s, `enum`s, etc.
> 
> **Implementation status:** This spec module is **partially implemented** in the
> Rust codebase at `src/orchestration/`. The state machine, completion handler,
> guards, and orphan recovery are implemented. Citations into `reference/` which
> do not yet have `src/` equivalents are retained as guidance.

This is the engine. Everything else in `core/` exists to serve the state
machine described here.

> NOTE: OpenCode is the built-in coding harness.

## What it delivers

> I describe a task in a markdown file and press **Run** once. A chain of agents
> (each a new harness session, with the appropriate model(s)) plans the work,
> implements it, reviews it, and opens a pull request —
> iterating between implementation and review on their own until the work passes
> or they hit something only I can resolve. I watch it happen live and step in
> only when I want to.

Autonomy is the point. Between the first Run and the open PR there is **no human
in the loop** unless an agent explicitly asks for one. The orchestrator's whole
job is to decide, each time an agent finishes, what should happen next.

## Vocabulary

- **Task** — a unit of work backed by a markdown document and an isolated git
  worktree. See [`task-and-workspace.md`](./task-and-workspace.md).
- **Agent** — a role (planning, implementation, review, PR) expressed as a
  prompt run on the OpenCode coding harness
- **Agent run** — one execution of one agent against one task: a row in
  `task_agent_runs`, linked to a conversation.
- **Conversation** — one streaming session with the OpenCode harness; how an agent run
  actually executes and persists its transcript. See
  [`opencode.md`](../extra/harnesses/opencode.md).
- **Workflow flags** — `workflow_blocked` and `workflow_run_count` on the task row.
  `workflow_blocked` is a **server-only** marker (set exclusively by the iteration-cap
  path); agents can no longer set it. The loop's routing otherwise reads the review
  verdict from the transcript (see below), not from flags.

## The core agent roster

Four agents make up the core pipeline. Each has its own spec; here is only what
the loop needs to know about them.

| Agent | Does | Signals "done" by (completion handler) |
|---|---|---|
| **planning** (`planification`) | Turns the task doc + original request into a structured plan, written back into the doc. Touches nothing but the doc. | Ending its turn — the completion handler stops (human gate). |
| **implementation** | Implements the unchecked to-do items from the plan, inside the worktree. | Ending its turn — the completion handler auto-advances to review. |
| **review** | Verifies the implementation against the plan, runs tests, and decides READY / NEEDS_WORK / BLOCKED. | Ending its turn with the final model message containing the keyword **`READY`** (case-sensitive) → finish pipeline (refinement/PR). Without `READY`, the handler bounces back to implementation. |
| **PR** (`pr`) | Opens the pull request, drives CI to green, resolves conflicts. Terminal. | Ending its turn — nothing chains after PR. |

The agent-type enum in the schema also contains `refinement` and `yolo`. Those
are **extras** ([`refinement-agent.md`](../extra/refinement-agent.md),
[`yolo-mode.md`](../extra/yolo-mode.md)). Core uses only the four above.

## The state machine

An agent run is started for a `(taskId, agentType)` pair in one of two ways:

- **Manually** — the user presses Run for a specific agent
  (`POST /api/tasks/:taskId/agent-runs`).
- **By chaining** — when a run finishes, the orchestrator decides the next agent
  and starts it.

Both paths converge on the same entry point and follow the same shape: create
the `task_agent_runs` row (status `running`) and a linked conversation,
increment the task's run counter, stream the agent's turn, and on stream end
invoke the **completion handler**. The completion handler is where all routing
lives.

```
                 ┌──────────────── manual Run ────────────────┐
                 ▼                                             │
            planning ───────────────────────▶ [STOP: human reviews plan] ──Run──┐
                                                                                │
   ┌────────────────────────────────────────────────────────────────────────────┘
   ▼
implementation ──▶ review ──┬─ last model msg contains "READY" ─▶ Refinement ──▶ PR
        ▲                   │             (or PR / Terminal if unconfigured)
        │                   │
        └───────────────────┴─ no "READY" ─────────────────────▶ implementation   (loop)

PR ────────────────────────────────────────▶ [TERMINAL]
```

The review verdict is a **simple, case-sensitive substring search** for the literal
`READY` in the review's **last model (assistant) message** in the transcript. No flag
endpoints, no completion scripts, no structured return data.

### Transitions, precisely

When a run's stream ends, the completion handler
([`reference/server/services/conversation/agentRunLifecycle.ts`](../reference/server/services/conversation/agentRunLifecycle.ts))
does this:

1. Find the agent run linked to the finished conversation.
   - Status still `running` → the turn ended normally → mark it `completed`,
     broadcast the update, and **chain**.
   - Status `failed` → the user already pressed Stop (Stop writes `failed`
     synchronously, before the stream ends) → do nothing, do not chain.
2. Chaining is decided by `next_agent()` in `src/orchestration/state_machine.rs`,
   reading the task state and (for review runs) the READY keyword:
   - **After planning:** STOP. The plan is a human gate — the user reads the
     plan and presses Run for implementation. (Auto-advancing past this gate for
     non-technical users is a role extra; see
     [`auth-and-multi-user.md`](../extra/auth-and-multi-user.md). Core always
     stops here.)
   - **If `workflow_blocked` is set:** STOP. The server-only iteration-cap marker
     is the only way this flag is written.
   - **If `workflow_run_count` ≥ the cap:** auto-block the task and STOP
     (broadcast `task-blocked`, reason `max_iterations`).
   - **Review finished, last model message contains `READY`:** finish pipeline →
     start the **refinement** agent if configured; else start the **PR** agent if
     configured; else **Terminal**.
   - **Review finished without `READY`:** bounce back to **implementation** (the
     loop continues) if implementation is configured; else STOP.
   - **Refinement finished:** chain to the **PR** agent if configured; else
     **Terminal**.
   - **Otherwise alternate the loop:** implementation → review, review →
     implementation.
   - **PR is terminal** — nothing chains after it.

### Why the loop alternates the way it does

The alternation is a plain toggle: `implementation`'s default next is `review`,
`review`'s default next is `implementation` unless the `READY` keyword diverts it
into the finish pipeline. A `review` whose final message contains `READY` proceeds
to refinement/PR; a `review` that signals NEEDS_WORK or BLOCKED simply omits the
keyword, and the toggle sends it back to `implementation` for another pass. The
`implementation` and `review` prompts use the task doc's "Review Findings" section
as their shared scratchpad across iterations — see
[`execution-loop.md`](./execution-loop.md).

## Agents signal the verdict by a keyword, not by running scripts

This is the central design decision and the easiest thing to get wrong. **An
agent's turn returns nothing structured, and there are no flag endpoints.** The
orchestrator decides the review verdict with a **single case-sensitive substring
search** of the review conversation's **last model (assistant) message** for the
literal keyword `READY`:

- **`READY` present** → the feature is approved → finish pipeline (refinement if
  configured, else PR if configured, else Terminal).
- **`READY` absent** → the task loops back to implementation.

The check runs in `completion_handler()` (`src/orchestration/mod.rs`) only when
the finished run's agent type is `review`. It loads the conversation's transcript
(`transcript::last_model_text`) and searches the last `ProviderEvent::Text` for
`READY`. User messages are `ProviderEvent::UserText`, so they are never confused
with model output. The keyword must be exactly `READY` — `ready`, `Ready`, and
`READY!` still match only on the literal substring, so the review prompt instructs
agents to end an approving final message with the exact token `READY`.

Previously agents flipped task flags by calling HTTP endpoints
(`complete-plan` / `complete-workflow` / `block-workflow` / `complete-pr`) using
credentials from a `.ofm_agent.json` file written into the worktree. Both the
endpoints and the file are **removed**; the browser gets its token from
`POST /api/auth/refresh` and machine access uses `OFM_API_KEY`. The `ofm agent`
CLI subcommand was removed along with them.

Separately, each time a turn is started or resumed, `OpenCodeSdkProvider` routes the task worktree path
to the opencode server as a `directory` **query param** on every workspace-scoped HTTP call
(`session.create`, `event.subscribe`, `session.prompt_async`, `session.abort`). `start()` re-scopes the
pooled client handle with `OpencodeClient::with_directory(&worktree)`, and the client methods in
`src/opencode_sdk/client.rs` append the param when set. This ensures the opencode server resolves the
correct working directory for the task — the server's own CWD is shared per user and cannot be per-task,
and a session-row PATCH is insufficient because opencode's `WorkspaceRoutingMiddleware` re-resolves the
directory per HTTP call.

The payoff: the orchestrator stays dumb and robust. It does not need to
understand what an agent decided — it only searches one transcript for one
keyword.

## Why completion is database-driven, not error-driven

The completion handler intentionally has **no "did it error?" input.** Whether a
run succeeded or failed is determined solely by what is already in the database
when the stream ends:

- A normal end leaves status `running` → treated as success → mark `completed` →
  chain.
- A user Stop writes status `failed` *before* the stream ends → handler sees
  `failed` → no chain.
- A **mid-stream provider/model error** also writes `failed` up front: the
  broadcast loop pre-marks the still-running agent run `failed` the instant a
  `ProviderEvent::Error` is seen (`fail_linked_agent_run` in
  `src/services/tasks.rs`, wired into both broadcast loops —
  `src/orchestration/mod.rs` and `src/server/routes/conversations.rs`). This
  ports the reference implementation's `failLinkedAgentRunIfRunning()`
  (`spec/reference/server/services/conversation/startOpenCodeConversation.ts`),
  and keeps the "no `isError` parameter" rule intact — the handler still only
  reads DB state. The stream runs to `Done`, the completion handler sees
  `status != running` → **no chain**, so a broken environment (bad provider
  credentials, model errors) can no longer burn the whole iteration cap in an
  Impl⇄Review auto-advance loop. The error is still broadcast and the
  transcript is preserved — the user can resume the conversation or start a
  manual run.
- A catastrophic harness crash also leaves status `running` → treated as
  "completed" → chains to the next agent, which reads the synthetic error
  message left in the transcript and decides whether to retry. Failures heal
  *inside* the loop instead of dead-ending it.

Read the header comment in
[`agentRunLifecycle.ts`](../reference/server/services/conversation/agentRunLifecycle.ts)
before reimplementing this — the "no `isError` parameter" rule is load-bearing,
and the obvious "pass success/failure into the handler" design is the wrong one.

## Concurrency and safety rails

- **One running agent per task.** A manual start returns HTTP 409 if one is
  already running; chaining re-checks "is an agent running for this task?"
  immediately before starting the next run and bails if something is live.
- **Settle before chaining.** Chaining starts the next run after a short delay
  (the reference uses a ~1s `setTimeout`; `ofm` can use `tokio::time::sleep()`
  and a 1,000 millisecond timeout) so the finishing turn's status write
  and broadcasts land first, and it **re-reads the task state inside that
  callback** — the task may have been completed or blocked in the gap.
- **Iteration cap.** Every run increments `workflow_run_count`. When it reaches
  the cap (reference: `MAX_WORKFLOW_RUNS = 25`) the loop auto-blocks the task
  rather than running forever. Manual chats do not count.
- **Orphan recovery on restart.** Agent runs are in-memory streams; a server
  restart orphans any row still marked `running`. On boot, sweep all `running`
  agent runs to `failed` so the UI isn't stuck and the loop can be re-triggered.
  See the recovery block near the top of
  [`reference/server/index.ts`](../reference/server/index.ts)
  (`agentRunsDb.getByStatus('running')`).

## The trigger surface

- **Start a run (manual):** `POST /api/tasks/:taskId/agent-runs` with
  `{ agentType }`. Returns 201 with the created run, 409 if one is already
  running, 403 if the user has no credentials for the harness this agent is
  configured to use. See
  [`reference/server/routes/agent-runs.ts`](../reference/server/routes/agent-runs.ts).
- **Start a run (chaining):** internal only. The completion handler calls the
  same entry point — there is no separate code path. Manual and chained starts
  converge on `startAgentRun` in
  [`reference/server/services/agentRunner.ts`](../reference/server/services/agentRunner.ts).
- Re-triggering the loop from a GitHub PR comment is an extra:
  [`pr-comment-retrigger.md`](../extra/pr-comment-retrigger.md).

### Recovery endpoints

When the cap auto-blocks a task (`workflow_blocked`) or the counter is spent,
the workflow is a dead end until the user intervenes. Three user-triggered
actions on a task provide recovery; all are `POST` under
`/api/tasks/:taskId/`, require write access, and broadcast a `task_updated`
event on the task topic afterward so open pages reload (wired at
`src/webapp/pages/task_detail.rs`):

- **`/reset-cap`** — zero `workflow_run_count` and clear `workflow_blocked` in
  one action. Keeps history, worktree, task id. "Keep this conversation and
  try again."
- **`/reset-history`** — abort in-flight turns, delete the task's agent runs,
  conversations, and message transcripts, and reset every workflow flag, the
  run counter, and `status` to a fresh `pending`. Keeps task id, title, doc,
  and worktree. "This conversation is poisoned, start over on the same issue."
- **`/duplicate`** — create a *new* task whose archive doc is a copy of the
  source doc, with a fresh worktree and zero counters/flags/conversations.
  Original untouched. Returns 201 with the new task. "Spin up a clean instance
  of the issue."

Handlers live in `src/server/routes/tasks.rs`; the services
(`reset_task_cap`, `reset_task_history`) and the error-pre-mark helper
(`fail_linked_agent_run`) live in `src/services/tasks.rs`.

### Blocked/cap UX

A blocked or capped task is surfaced in the UI:

- **Task detail / chat pages** render a `notification is-danger` recovery
  banner (`src/webapp/components/task_recovery.rs`) when
  `workflow_blocked` or `workflow_run_count >= MAX_WORKFLOW_RUNS`, explaining
  the cause, showing an `Agent runs: n/25` tag, and offering the three buttons
  above.
- **Board cards** (`src/webapp/components/task_card.rs`) show a "Blocked" tag.
- **Manual run 409s** are distinguished in the run-button JS
  (`src/webapp/components/conversation_list.rs`): a `max iterations reached`
  body shows "This task hit the max agent-run cap. Reset the cap from the task
  page." instead of "Agent already running".
- **Cap auto-block** broadcasts a `task_updated` event live from
  `completion_handler` (`src/orchestration/mod.rs`), so open pages show the
  banner without a manual reload.

## What `startAgentRun` is responsible for

One function, in order (see `start_next_agent()` in `src/orchestration/mod.rs`):

1. Resolve the task and its effective working directory (the worktree if it
   exists, else the repo path).
2. Build the agent's prompt for `agentType` from the task doc — and, for the PR
   agent, the current PR status. Prompt design lives in each agent's spec.
3. Increment the task's run counter.
4. Create the `task_agent_runs` row (`running`) and a linked conversation.
5. Flip task status `pending → in_progress` on first activity.
6. **Start the provider** with the worktree path as the working directory
   (resolved in step 1). This must happen *after* worktree resolution so that
   `start()` scopes the client to the correct worktree via `with_directory()`
   (which routes the `directory` query param per HTTP call). See `src/orchestration/mod.rs`.
7. Start the conversation/turn through the harness contract, wiring the
   completion handler as the stream's on-complete hook.

### Footprint plumbing

The `start_next_agent()` function receives the `footprint` (OFM footprint path)
and passes it through to the provider and pool chain:
- `registry::resolve_provider_for_user()` receives `footprint` and passes it to
  the `OpenCodeSdkProvider` constructor.
- The provider passes `footprint` to `OpenCodeServerPool::get_or_spawn()`, which
  templates the `external_directory` allowlist in the opencode server config.
- See `src/providers/opencode_sdk_provider.rs`, `src/opencode_sdk/pool.rs`.

### Prompt context: working directory and allowed paths

The context prompt built by `build_context_prompt()` in `src/archive/mod.rs` now
includes a **Working Directory** section that tells the agent:
- Its authoritative working directory (the worktree path).
- Which paths it is allowed to read/write: the worktree, the archive (for task
  docs), and `/tmp/` for scratch files.
- That it MUST NOT write anywhere else.

The model and credential resolution that step 6 depends on are an
extra ([`prompt-and-model-customization.md`](../extra/prompt-and-model-customization.md)).
The direct harness integration that step calls is in [`opencode.md`](../extra/harnesses/opencode.md).

## Build checklist

- [x] Task state on the task row: `workflow_blocked`, `workflow_run_count`
      (plus `status`). See `src/db/schema.rs` (`Task` struct).
- [x] `task_agent_runs` table: `(task_id, agent_type, status, conversation_id)`,
      status in `pending | running | completed | failed | blocked`.
      See `src/db/schema.rs` (`TaskAgentRun` struct).
- [x] `startAgentRun(taskId, agentType)` — the single entry point for manual and
      chained starts. See `src/server/routes/agent_runs.rs` (`post_create_agent_run`).
- [x] A completion handler wired as the streaming on-complete hook, implementing
      the transitions above. See `src/orchestration/mod.rs` (`completion_handler`).
- [x] The READY keyword review flow (last-model-message search + `next_agent`
      transitions). See `src/orchestration/state_machine.rs`, `src/services/transcript.rs`.
- [x] The "one running agent per task" guard (manual 409 + pre-chain re-check).
      See `src/orchestration/guards.rs`.
- [x] The iteration cap and auto-block. See `src/orchestration/guards.rs`,
      `src/orchestration/state_machine.rs`.
- [x] Orphan-run recovery on startup. See `src/orchestration/recovery.rs`.
- [x] `POST /tasks/:taskId/agent-runs` plus a list endpoint.
      See `src/server/routes/agent_runs.rs`.
- [x] Error → `failed` pre-marking in both broadcast loops (halts the
      runaway loop on a broken environment). See `src/services/tasks.rs`
      (`fail_linked_agent_run`), `src/orchestration/mod.rs`,
      `src/server/routes/conversations.rs`.
- [x] Recovery endpoints `POST /tasks/:taskId/reset-cap`, `reset-history`,
      `duplicate`, plus the blocked/cap banner, board "Blocked" tag, and the
      friendlier 409 message. See `src/server/routes/tasks.rs`,
      `src/webapp/components/task_recovery.rs`,
      `src/webapp/components/task_card.rs`,
      `src/webapp/components/conversation_list.rs`.

## Reference map

| Concern | Rust (implemented) | Legacy reference |
|---|---|---|
| Start and own a run | `src/server/routes/agent_runs.rs` | `reference/server/services/agentRunner.ts` |
| Completion + chaining | `src/orchestration/mod.rs` | `reference/server/services/conversation/agentRunLifecycle.ts` |
| State machine / transitions | `src/orchestration/state_machine.rs` | — |
| READY keyword helper | `src/services/transcript.rs` (`last_model_text`) | — |
| Guards (concurrency, cap) | `src/orchestration/guards.rs` | — |
| Manual trigger HTTP | `src/server/routes/agent_runs.rs` | `reference/server/routes/agent-runs.ts` |
| Tables | `src/db/schema.rs` | `reference/server/database/init.sql` |
| Orphan recovery | `src/orchestration/recovery.rs` | `reference/server/index.ts` |
| Provider abstraction | `src/providers/` (`LlmProvider` trait, registry, config) | — |

## Boundaries (intentionally not in this spec)

- The plan's content and the implementation/review prompt design →
  [`planning-agent.md`](./planning-agent.md),
  [`execution-loop.md`](./execution-loop.md).
- How a turn actually streams and persists its transcript →
  [`opencode.md`](../extra/harnesses/opencode.md).
- The refinement step, YOLO single-agent mode, model/effort selection, the
  non-technical auto-advance, the task-authoring board, and webhook re-trigger →
  the corresponding `extra/` specs.


