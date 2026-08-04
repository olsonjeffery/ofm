# Extra — The refinement agent

> **⚠️`ofm` ONLY ⚠️:** Rust convention requires functions and `let` bindings
> use `snake_case` as a naming convention. In all places where `camelCase`
> occurs (in citations from the legacy typescript `reference/` implementation),
> substitute for `snake_case` as appropriate; `PascalCase` is used for `trait`s,
> `struct`s, `enum`s, etc.
> 
> **Implementation status:** This extra is **not yet implemented** in the Rust
> codebase — the refinement prompt/harness do not exist yet. However, the
> READY-driven routing that would invoke it **is** implemented: when a review's
> last model message contains `READY`, `next_agent()` in
> `src/orchestration/state_machine.rs` starts the **refinement** agent if a
> refinement harness is configured, else the PR agent if configured, else
> Terminal. Wiring a refinement harness config is all that remains.

## What it adds

A single optional polish pass that runs **after review approves the work and
before the PR is opened**. When review signals READY (the `READY` keyword in the
review's last model message), instead of going straight to the PR agent, the
loop first runs a **refinement** agent that cleans up the just-written code: it
simplifies the diff for clarity and runs a security pass over it, applying fixes
in place. Only then does the PR agent run. The reviewed work ships a little
tidier and a little safer, without a human in the loop.

## Why it's an extra (not core)

Shipping a reviewed change as a PR is universal; an extra cleanup pass before
that PR is a matter of taste. Remove this extra (i.e. configure no refinement
harness) and the finish pipeline goes straight `READY → PR`, exactly as core
describes.

## Where it inserts

The finish pipeline lives in `next_agent()` in
[`../core/orchestration-loop.md`](../core/orchestration-loop.md). The review
branch reads the `READY` keyword and decides, in order:

- refinement harness configured → start the **refinement** agent.
- else PR harness configured → start the **PR** agent.
- else → **Terminal**.

When the finishing agent **was** refinement, the handler simply falls through to
the PR check and starts the PR agent. So a single refinement run is threaded in:
review → refinement → PR. Refinement is one of the agent types that the
completion handler treats as chainable (alongside planning, implementation,
review). The PR agent remains terminal.

Because routing is driven by configuration rather than a `refinement_complete`
flag, a second review that signals `READY` **after** refinement has run would
re-run refinement. This is an acceptable divergence for a polish pass; if
"at most once per task" is desired, add a persistent marker and check it in the
review branch.

## What the agent actually does

The prompt is [`../reference/server/constants/prompts/refinement.md`](../reference/server/constants/prompts/refinement.md).
It is built like any other agent message — `generateRefinementMessage`
(taskDocPath + taskId) in
[`../reference/server/constants/agentPrompts.ts`](../reference/server/constants/agentPrompts.ts)
renders it through the prompt engine — and runs through the standard
`startAgentRun` path in
[`../reference/server/services/agentRunner.ts`](../reference/server/services/agentRunner.ts)
(the `case 'refinement'` branch).

The agent works in three steps:

1. **Spawn two sub-tasks in parallel** in a single turn (it uses the Agent/Task
   tool here — refinement is *not* in the sub-agent-disallowed set, unlike
   implementation and yolo):
   - **Code simplification** — diff `main` to find the modified files, read
     them, and simplify for clarity: drop unnecessary complexity, improve
     naming, reduce duplication, simplify conditionals. Behavior must be
     preserved; only files changed on this branch may be touched; test files
     and the task doc are off-limits. This sub-task **applies its changes
     directly.**
   - **Security review** — a read-only OWASP-style pass over the diff that
     produces a report of HIGH/MEDIUM findings at confidence ≥ 8, each with
     file, line, severity, and a recommended fix. It **modifies nothing**; it
     only reports.
2. **Apply the security fixes** — after both sub-tasks return, the parent reads
   the security report and applies each qualifying fix to the affected files.
3. **Log a short summary** — counts of simplifications and security fixes and
   the files touched.

Hard constraints baked into the prompt: never edit the task doc, never run a
completion script, never ask questions, never run tests (the PR agent owns CI).
Refinement signals "done" simply by ending its turn — there is no refinement
script or flag. The orchestrator routes to the PR agent when refinement ends
(`next_agent` chains refinement → PR).

## When to install it

Install refinement when you want an automated tidy-and-harden step on every
reviewed change before it becomes a PR — and you accept that it costs an extra
agent run (and a sub-agent fan-out) per task, and that it edits code after the
reviewer already blessed it. Skip it if you'd rather the reviewed diff reach the
PR untouched.

## What to build

- [ ] The refinement prompt: spawn the two parallel sub-tasks (simplification +
      read-only security review), then apply the security fixes, then summarize
      — with the "no doc edits / no scripts / no tests / no questions"
      constraints.
- [ ] A `refinement` branch in the agent-message builder and in `startAgentRun`
      (no `disallowedTools` restriction — refinement *needs* the sub-agent
      tool).
- [ ] `refinement` in the chainable agent-type set so the completion handler
      routes after it (already implemented: `next_agent` chains refinement → PR).

## Reference map

| Concern | Rust (implemented) | Legacy reference |
|---|---|---|
| Routing (READY → refinement → PR) | `src/orchestration/state_machine.rs` (`next_agent`) | `../reference/server/services/conversation/agentRunLifecycle.ts` |
| What the agent does (prompt) | — (not yet implemented) | `../reference/server/constants/prompts/refinement.md` |
| Message assembly | `src/agents/refinement.rs` (`build_refinement_prompt`) | `../reference/server/constants/agentPrompts.ts` |
| Run start (no sub-agent ban) | `src/orchestration/mod.rs` (`start_next_agent`) | `../reference/server/services/agentRunner.ts` |

## Boundaries (not in this spec)

- The state machine the refinement check lives inside (chaining, the READY
  keyword routing, the iteration cap) →
  [`../core/orchestration-loop.md`](../core/orchestration-loop.md).
- The review step that signals READY upstream →
  [`../core/execution-loop.md`](../core/execution-loop.md).
- The PR agent that runs after refinement →
  [`../core/pull-request-agent.md`](../core/pull-request-agent.md).
- The single-agent path that skips this pipeline entirely →
  [`./yolo-mode.md`](./yolo-mode.md).
