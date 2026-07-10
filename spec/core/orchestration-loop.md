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

> NOTE: [`oh-my-pi`](https://omp.sh) is the primary coding harness.

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
  prompt run on a coding harness (e.g. `oh-my-pi`, OpenCode)
- **Agent run** — one execution of one agent against one task: a row in
  `task_agent_runs`, linked to a conversation.
- **Conversation** — one streaming session with a harness (`oh-my-pi`, OpenCode); how an agent run
  actually executes and persists its transcript. See
  [`oh-my-pi.md`](../extra/harnesses/oh-my-pi.md) and [`opencode.md`](../extra/harnesses/opencode.md).
- **Workflow flags** — booleans on the task row that gate the loop. They are the
  orchestrator's entire memory of "where are we."

## The core agent roster

Four agents make up the core pipeline. Each has its own spec; here is only what
the loop needs to know about them.

| Agent | Does | Signals "done" by (completion handler) |
|---|---|---|
| **planning** (`planification`) | Turns the task doc + original request into a structured plan, written back into the doc. Touches nothing but the doc. | Running `complete-plan.ts` (`bottega`) OR `ofm agent complete-plan <task-id>` → sets `planification_complete`. |
| **implementation** | Implements the unchecked to-do items from the plan, inside the worktree. | Ending its turn. No script — completion is implicit. |
| **review** | Verifies the implementation against the plan, runs tests, and decides READY / NEEDS_WORK / BLOCKED. | READY → `complete-workflow.ts` (`bottega`) OR `ofm agent complete-workflow <task-id>` (sets `workflow_complete`). BLOCKED → `block-workflow.ts` (`bottega`) OR `ofm agent block-workflow <task-id>` (sets `workflow_blocked`). NEEDS_WORK → no script, just ends. |
| **PR** (`pr`) | Opens the pull request, drives CI to green, resolves conflicts. Terminal. | Running `complete-pr.ts` (`bottega`) OR `ofm agent complete-pr` → sets `pr_agent_complete`. |

The agent-type enum in the schema also contains `refinement` and `yolo`. Those
are **extras** ([`refinement-agent.md`](../extra/refinement-agent.md),
[`yolo-mode.md`](../extra/yolo-mode.md)). Core uses only the four above.

Map all of these onto [`oh-my-pi`][4] subagents

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
            planning ──(complete-plan)──▶ [STOP: human reviews plan] ──Run──┐
                                                                            │
   ┌────────────────────────────────────────────────────────────────────────┘
   ▼
implementation ──▶ review ──┬─ NEEDS_WORK ───────────────────────▶ implementation   (loop back)
       ▲                    │
       │                    ├─ READY  (complete-workflow → workflow_complete) ──▶ PR
       │                    │
       └────────────────────┴─ BLOCKED (block-workflow → workflow_blocked) ──▶ [STOP: human]

PR ──(complete-pr → pr_agent_complete)──▶ [TERMINAL]
```

### Transitions, precisely

When a run's stream ends, the completion handler
([`reference/server/services/conversation/agentRunLifecycle.ts`](../reference/server/services/conversation/agentRunLifecycle.ts))
does this:

1. Find the agent run linked to the finished conversation.
   - Status still `running` → the turn ended normally → mark it `completed`,
     broadcast the update, and **chain**.
   - Status `failed` → the user already pressed Stop (Stop writes `failed`
     synchronously, before the stream ends) → do nothing, do not chain.
2. Chaining is decided **only from task flags**, never from anything the agent
   "returned":
   - **After planning:** STOP. The plan is a human gate — the user reads the
     plan and presses Run for implementation. (Auto-advancing past this gate for
     non-technical users is a role extra; see
     [`auth-and-multi-user.md`](../extra/auth-and-multi-user.md). Core always
     stops here.)
   - **If `workflow_complete` is set** (review ran `complete-workflow.ts`): enter
     the finish pipeline → start the **PR** agent. (The refinement extra inserts
     itself here, *before* PR.)
   - **If `workflow_blocked` is set:** STOP. A human must resume.
   - **If `workflow_run_count` ≥ the cap:** auto-block the task and STOP
     (broadcast `task-blocked`, reason `max_iterations`).
   - **Otherwise alternate the loop:** implementation → review, review →
     implementation.
   - **PR is terminal** — nothing chains after it.

### Why the loop alternates the way it does

The alternation is a plain toggle: `implementation`'s default next is `review`,
`review`'s default next is `implementation`. The crucial detail is *ordering* — the
`workflow_complete` check runs **before** the toggle. So a `review` that signals
READY (by running `complete-workflow.ts`) diverts into the finish pipeline
instead of bouncing back to `implementation`; a `review` that signals NEEDS_WORK
simply doesn't set the flag, and the toggle sends it back to `implementation` for
another pass. The `implementation` and `review` prompts use the task doc's "Review
Findings" section as their shared scratchpad across iterations — see
[`execution-loop.md`](./execution-loop.md).

## Agents signal state by running scripts, not by returning data

This is the central design decision and the easiest thing to get wrong. **An
agent's turn returns nothing structured.** The orchestrator never parses the
model's prose for a verdict. Instead, agents are instructed (in their prompts)
to run small CLI scripts that flip task flags, and the completion handler reads
those flags after the turn ends.

| Script | Flag set | Run by |
|---|---|---|
| [`reference/scripts/complete-plan.ts`](../reference/scripts/complete-plan.ts) (`bottega`) OR `ofm agent complete-plan <task-id>` | `planification_complete` | planning agent |
| [`reference/scripts/complete-workflow.ts`](../reference/scripts/complete-workflow.ts) (`bottega`) OR `ofm agent complete-workflow <task-id>` | `workflow_complete` | review agent, on READY |
| [`reference/scripts/block-workflow.ts`](../reference/scripts/block-workflow.ts) (`bottega`) OR `ofm agent block-workflow <task-id>` | `workflow_blocked` | review agent, on BLOCKED |
| [`reference/scripts/complete-pr.ts`](../reference/scripts/complete-pr.ts) (`bottega`) OR `ofm agent complete-pr <task-id>` | `pr_agent_complete` | PR agent |

Each script is tiny: validate the task id, flip one boolean, exit. They run
inside the agent's own sandbox (`bottega`; the agent has shell access) OR
via the `ofm agent <action> <task-id>` command against the same
database the server uses. Build them as standalone entry points in `ofm`
an agent can invoke as `ofm agent <action> <task-id>`; the agent should
have access to the `ofm` bin.

The payoff: the orchestrator stays dumb and robust. It does not need to
understand what an agent decided — it only reads four booleans.

## Why completion is database-driven, not error-driven

The completion handler intentionally has **no "did it error?" input.** Whether a
run succeeded or failed is determined solely by what is already in the database
when the stream ends:

- A normal end leaves status `running` → treated as success → mark `completed` →
  chain.
- A user Stop writes status `failed` *before* the stream ends → handler sees
  `failed` → no chain.
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
  and broadcasts land first, and it **re-reads the task flags inside that
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

## What `startAgentRun` is responsible for

One function, in order (study
[`reference/server/services/agentRunner.ts`](../reference/server/services/agentRunner.ts)):

1. Resolve the task and its effective working directory (the worktree if it
   exists, else the repo path).
2. Build the agent's prompt for `agentType` from the task doc — and, for the PR
   agent, the current PR status. Prompt design lives in each agent's spec.
3. Increment the task's run counter.
4. Create the `task_agent_runs` row (`running`) and a linked conversation.
5. Flip task status `pending → in_progress` on first activity.
6. Start the conversation/turn through the harness contract, wiring the
   completion handler as the stream's on-complete hook.

The model and credential resolution that step 6 depends on are an
extra ([`prompt-and-model-customization.md`](../extra/prompt-and-model-customization.md)).
The direct harness integration that step calls is in [`oh-my-pi.md`](../extra/harnesses/oh-my-pi.md) and [`opencode.md`](../extra/harnesses/opencode.md).

## Build checklist

- [x] Task flags on the task row: `workflow_complete`, `workflow_blocked`,
      `workflow_run_count`, `planification_complete`, `pr_agent_complete`
      (plus `status`). See `src/db/schema.rs` (`Task` struct).
- [x] `task_agent_runs` table: `(task_id, agent_type, status, conversation_id)`,
      status in `pending | running | completed | failed | blocked`.
      See `src/db/schema.rs` (`TaskAgentRun` struct).
- [x] `startAgentRun(taskId, agentType)` — the single entry point for manual and
      chained starts. See `src/server/routes/agent_runs.rs` (`post_create_agent_run`).
- [x] A completion handler wired as the streaming on-complete hook, implementing
      the transitions above. See `src/orchestration/mod.rs` (`completion_handler`).
- [x] The four signalling actions under `ofm agent ...`
      See `src/server/routes/agent_flags.rs`.
- [x] The "one running agent per task" guard (manual 409 + pre-chain re-check).
      See `src/orchestration/guards.rs`.
- [x] The iteration cap and auto-block. See `src/orchestration/guards.rs`,
      `src/orchestration/state_machine.rs`.
- [x] Orphan-run recovery on startup. See `src/orchestration/recovery.rs`.
- [x] `POST /tasks/:taskId/agent-runs` plus a list endpoint.
      See `src/server/routes/agent_runs.rs`.

## Reference map

| Concern | Rust (implemented) | Legacy reference |
|---|---|---|---|
| Start and own a run | `src/server/routes/agent_runs.rs` | `reference/server/services/agentRunner.ts` |
| Completion + chaining | `src/orchestration/mod.rs` | `reference/server/services/conversation/agentRunLifecycle.ts` |
| State machine / transitions | `src/orchestration/state_machine.rs` | — |
| Guards (concurrency, cap) | `src/orchestration/guards.rs` | — |
| Manual trigger HTTP | `src/server/routes/agent_runs.rs` | `reference/server/routes/agent-runs.ts` |
| Flags + tables | `src/db/schema.rs` | `reference/server/database/init.sql` |
| Signalling actions | `src/server/routes/agent_flags.rs` | `reference/scripts/{complete-plan,complete-workflow,block-workflow,complete-pr}.ts` |
| Orphan recovery | `src/orchestration/recovery.rs` | `reference/server/index.ts` |
| Provider abstraction | `src/providers/` (`LlmProvider` trait, registry, config) | — |

## Boundaries (intentionally not in this spec)

- The plan's content and the implementation/review prompt design →
  [`planning-agent.md`](./planning-agent.md),
  [`execution-loop.md`](./execution-loop.md).
- How a turn actually streams and persists its transcript →
  [`oh-my-pi.md`](../extra/harnesses/oh-my-pi.md) and [`opencode.md`](../extra/harnesses/opencode.md).
- The refinement step, YOLO single-agent mode, model/effort selection, the
  non-technical auto-advance, the task-authoring board, and webhook re-trigger →
  the corresponding `extra/` specs.

[4]: https://omp.sh/docs/subagents
