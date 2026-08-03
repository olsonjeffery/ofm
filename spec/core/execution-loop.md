# Core — The execution loop

> **⚠️`ofm` ONLY ⚠️:** Rust convention requires functions and `let` bindings
> use `snake_case` as a naming convention. In all places where `camelCase`
> occurs (in citations from the legacy typescript `reference/` implementation),
> substitute for `snake_case` as appropriate; `PascalCase` is used for `trait`s,
> `struct`s, `enum`s, etc.
> 
> **Implementation status:** This spec module is **partially implemented** in the
> Rust codebase. Prompt builders exist at `src/agents/implementation.rs` and
> `src/agents/review.rs`; the orchestration state machine at
> `src/orchestration/state_machine.rs` handles turn lifecycle. The full
> implementation/review agent loop wiring (completion handler chaining,
> Review Findings scratchpad, iteration-aware prompts) is not yet complete.
> `reference/` citations are retained for those parts.

The autonomous heart of the pipeline: an **implementation** agent does the work
from the plan, a **review** agent independently verifies it, and they alternate
until the work is verified ready — or the loop hits something only a human can
resolve. No person acts in between.

## What it delivers

> Once I approve the plan, two agents take turns: one writes the code and checks
> off the plan's to-do items, the other independently checks the work against
> the plan and runs the tests. If the reviewer finds gaps, it sends the work
> back; if it's satisfied, it releases the task to the PR step. I don't touch
> anything unless they get stuck.

## The shared scratchpad: the task doc

The two agents are separate, stateless turns — they don't share memory. They
coordinate **entirely through the task doc**. That one file carries:

- the plan's **To-Do List** (`Implementation` + `Testing` checkboxes), and
- a **Review Findings** section the review agent writes and the implementation
  agent reads.

This is the whole protocol: implementation checks items off and addresses
findings; review verifies checked items and rewrites the findings. State lives
in the document, not in either agent's context.

## The implementation agent

- Reads the task doc. **If a Review Findings section exists, address those
  issues first** — they're feedback from the previous review pass.
- Implements the **unchecked** to-do items in the worktree, marking each `[x]`
  as it finishes.
- **Does not ask questions** — it proceeds autonomously. Ambiguity was meant to
  be resolved at planning time.
- **Cannot delegate to sub-agents.** The Agent/sub-agent tool is disallowed for
  this agent so all work stays in one observable conversation rather than
  vanishing into an opaque, hours-long sub-agent. See the `disallowedTools` set
  in [`reference/server/services/agentRunner.ts`](../reference/server/services/agentRunner.ts).
- Ends its turn with **no completion script**. Completion is implicit, so the
  loop chains straight to review.

## The review agent

Independent quality assurance. Its prompt is deliberately skeptical —
implementation agents tend to mark things done when the work is partial, so the
reviewer's job is to catch that.

- **Early return.** If *any* to-do item is still unchecked, the implementation
  isn't finished. Write Review Findings = `IN_PROGRESS` listing the remaining
  items, stop, and return control to implementation. Don't review half-done
  work or run tests yet.
- **Full review (all items checked).** Verify **every** checked item against the
  plan with **strict matching** — "plan said create file X" means file X must
  exist with that content; an item checked but not actually done is a critical
  finding. Then run the tests: targeted unit tests first, then the full suite
  (long suites run in the background), then the manual scenarios from the
  Testing Strategy.
- **Verdict** — exactly one of:
  - **READY** — all items verified, all tests pass → end the review's **final
    model message with the exact keyword `READY`** (uppercase, case-sensitive).
    The orchestrator detects it and the loop enters the finish pipeline
    (→ refinement if configured, else → PR).
  - **NEEDS_WORK** — any verification or test failed → rewrite Review Findings
    with the specific issues, **uncheck the failed to-do items** (so
    implementation retries them), and end **without** the `READY` keyword. The
    loop chains back to implementation.
  - **BLOCKED** — all agent-doable work is done but remaining items physically
    require a human (a decision, external infra, credentials) → end **without**
    the `READY` keyword and document the blocker. The loop chains back to
    implementation, which hands off for user input; there is no `workflow_blocked`
    endpoint anymore — that flag is written only by the server-side iteration cap.
- Review **only documents and decides — it never fixes code.**
- It **replaces** the Review Findings section every time (no history kept).

## How the loop reads all this

The transitions live in [`orchestration-loop.md`](./orchestration-loop.md); in
brief: implementation → review, review → implementation, with diversions that
the loop checks **before** that toggle —

- review's last model message contains `READY` → finish pipeline
  (refinement if configured, else PR if configured, else terminal);
- `workflow_blocked` set (server-only iteration-cap marker) → stop;
- and the iteration cap auto-blocks a runaway loop.

So a NEEDS_WORK or BLOCKED review (which omits `READY`) simply falls through to
the toggle and bounces back to implementation.

## Why two agents

Separation of powers. The implementer is biased toward declaring victory; an
independent reviewer with a skeptical prompt and strict matching catches partial
or skipped work. Crucially, the verdict is expressed as **edits to the task doc
plus the `READY` keyword** — never as structured data the orchestrator has to
parse (the READY search is a plain case-sensitive substring check on the last
model message). That is what keeps the loop dumb and reliable (see
[`orchestration-loop.md`](./orchestration-loop.md)).

## What to build

- [x] The implementation prompt: address findings → implement unchecked items →
      check them off → never ask questions.
      → `templates/implementation.md`, `src/agents/implementation.rs`
- [ ] Disallow sub-agent delegation for the implementation agent.
- [x] The review prompt: early-return guard → strict per-item verification →
      unit + manual tests → READY / NEEDS_WORK / BLOCKED → replace findings,
      uncheck failed items, never fix code, signal READY via the `READY` keyword.
      → `templates/review.md`, `src/agents/review.rs`
- [x] The READY keyword detection: `completion_handler()` reads the review's last
      model message and searches for `READY`.
      → `src/services/transcript.rs` (`last_model_text`), `src/orchestration/mod.rs`

> The reference also records a Playwright video of the review's manual testing
> for the user to watch (the `videoConfig` wired up in `agentRunner.ts`). That's
> a nicety, not load-bearing — skip it if your harness has no browser-driving
> MCP.



## Reference map

| Concern | Rust (implemented) | Legacy reference |
|---|---|---|
| Implementation prompt | `templates/implementation.md`, `src/agents/implementation.rs` | `reference/server/constants/prompts/implementation.md` |
| Review prompt | `templates/review.md`, `src/agents/review.rs` | `reference/server/constants/prompts/review.md` |
| READY keyword detection | `src/orchestration/mod.rs`, `src/services/transcript.rs` | — |
| Prompt assembly + tool/video setup | — | `reference/server/constants/agentPrompts.ts`, `reference/server/services/agentRunner.ts` |

## Boundaries (not in this spec)

- The chaining mechanics, the iteration cap, and how the READY keyword routes
  the loop → [`orchestration-loop.md`](./orchestration-loop.md).
- A polishing pass between review and PR →
  [`refinement-agent.md`](../extra/refinement-agent.md).
- Opening the PR after READY → [`pull-request-agent.md`](./pull-request-agent.md).
