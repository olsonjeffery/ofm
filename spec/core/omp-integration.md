# Core — omp integration

This is the seam between the orchestration engine and the coding agent. Every
agent run — planning, the implementation/review loop, the PR agent, every manual
chat — executes by driving an [`omp`](https://omp.sh) subprocess in **RPC mode**
over `STDIO`. There is no provider abstraction: `omprint` is bound to `omp`, and
this spec describes that binding directly.

> NOTE: `omp` and [`oh-my-pi`](https://omp.sh) refer to the same coding agent.
> The binary is `omp`; it runs as a single process and is driven headlessly via
> `omp --mode rpc`.

## Why it is core

The orchestration loop and the agents are inert without *something* that runs a
coding-agent turn. That something is always `omp`. Because there is exactly one
backend, the integration is concrete: there is **no `LlmProvider` interface, no
provider registry, no capability matrix, no `UnifiedMessage` conversion layer**.
`omprint` spawns `omp`, speaks its native RPC protocol, and stores its native
events. The whole product reads `omp`'s event stream directly.

One rule keeps the seam clean: **everything above the RPC client speaks omp's
native frame shapes.** The framing/parsing of the STDIO channel is the only
place that knows about newline-delimited JSON; consumers above it read parsed
frames.

## The omp RPC protocol — what we are integrating against

`omp --mode rpc` runs the agent as a **newline-delimited JSON (NDJSON / JSONL)**
protocol over standard I/O. This is the authoritative shape `omprint` builds to.
Source of truth: omp's [RPC Protocol Reference](https://omp.sh/docs) (mirror:
`can1357/oh-my-pi` `docs/rpc.md`).

- **stdin** (omprint → omp): `RpcCommand` objects, extension-UI responses, and
  host-tool / host-URI results.
- **stdout** (omp → omprint): a startup `ready` frame, `RpcResponse` objects,
  streaming `AgentSessionEvent` objects, extension-UI requests, host-tool /
  host-URI requests, and side-channel frames.

Each frame is **one JSON object followed by `\n`**. There is no envelope beyond
the object shape itself; the object's `type` field discriminates it.

### Startup and shutdown

- Launch with `omp --mode rpc [cli options]`. `@file` CLI arguments are rejected
  in RPC mode.
- At startup omp writes `{ "type": "ready" }` **before** processing any command.
  `omprint` must wait for the ready frame before sending the first command.
- RPC mode **disables automatic session-title generation** by default (avoids an
  extra model call) and **resets** workflow-altering settings (`todo.*`,
  `task.*`, `memory.backend`/`memories.enabled`, `advisor.*`, `async.*`,
  `bash.autoBackground.*`) to built-in defaults rather than inheriting user
  config.
- When stdin closes, omp rejects pending host-tool/host-URI calls and **exits 0**.
  Closing our write half of the pty is the graceful-shutdown signal.

### Request/response correlation

Every command accepts an optional `id?: string`. If present, the matching
`RpcResponse` echoes the same `id`. `omprint` generates a unique id per command
(e.g. `req_<n>`) and resolves the pending request when the correlated response
arrives. Edge behavior to honor:

- Unknown-command responses come back with `id: undefined` even if the request
  carried one.
- Parse/handler exceptions in omp's input loop emit `{ command: "parse",
  success: false, id: undefined }` and omp **keeps reading** subsequent lines.
- `prompt` and `abort_and_prompt` are **acknowledged immediately** — the success
  response means *accepted*, not *turn finished*. Completion is signalled by
  session events (below), not by the command response.

## 1. Subprocess lifecycle

`omprint` owns the `omp` process. It is spawned through the
[`portable-pty`](https://github.com/wezterm/wezterm/tree/main/pty) crate (the
same pty machinery `omprint` uses for `git`, `rauthy`, and tool onboarding — see
[`SPEC.md`](../SPEC.md)), and a background `tokio` worker owns its STDIO.

- **One subprocess per conversation/turn-context.** Each agent run drives its own
  `omp` process, spawned with `cwd` set to the task's effective working directory
  (the worktree if present, else the repo path — resolved in `startAgentRun`, see
  [`task-and-workspace.md`](./task-and-workspace.md)). A process maps to one omp
  session; resuming a conversation re-spawns omp pointed at the stored session
  file (see §3).
- **Spawn arguments.** `omp --mode rpc` plus the per-run CLI options the agent
  needs: model/role selection, system-prompt injection, permission posture, and
  any tool disables (see §8). Credentials are supplied via the spawn environment
  (see §7).
- **Ready handshake.** The worker spawns omp, then blocks reading stdout until the
  `{ "type": "ready" }` frame lands before issuing the first command.
- **Shutdown.** Normal completion: close stdin (drop the pty writer); omp exits 0.
  Forced shutdown (task delete, server drain): close stdin, then escalate via the
  pty (SIGINT → SIGTERM → SIGKILL on a bounded timer) if the process does not
  exit.
- **Crash recovery.** A catastrophic omp crash (process dies, stdout EOF without a
  terminal event) is surfaced to the orchestration loop as a synthetic error
  result so the run ends rather than hanging — the orchestration loop's
  database-driven completion then lets the *next* agent read the error and decide
  whether to retry (see [`orchestration-loop.md`](./orchestration-loop.md), "Why
  completion is database-driven"). omp sessions persist to disk under
  `~/.omp/agent/sessions/`, so a crashed turn can be resumed from its last
  durable point rather than lost.

## 2. RPC communication — the STDIO channel

The background worker is a bidirectional pump over the pty:

- **Outbound (commands).** Serialize each `RpcCommand` to one JSON line + `\n` and
  write it to omp's stdin. Maintain a map of in-flight `id → oneshot` so the
  correlated `RpcResponse` resolves the caller. A `tokio::sync::mpsc` queue feeds
  the writer so concurrent callers (e.g. an abort racing a steer) serialize
  cleanly onto the single stdin.
- **Inbound (frames).** Read omp's stdout line by line. Each line is parsed as one
  JSON frame and dispatched by `type`:
  - `response` → resolve the correlated pending command (or drop if `id` unknown).
  - `ready` → release the startup handshake.
  - `AgentSessionEvent` (`agent_start`, `message_update`, `agent_end`, …) → feed
    the event consumer (§4): persist, broadcast, update context usage.
  - `extension_ui_request` → answer per the UI sub-protocol (§9).
  - `host_tool_call` / `host_tool_cancel`, `host_uri_request` / `host_uri_cancel`
    → serve host-owned tools/URIs if `omprint` registers any (§9, future).
  - side-channels (`available_commands_update`, `prompt_result`,
    `subagent_*`, `command_output`, `session_info_update`, `config_update`,
    `extension_error`) → consume as needed; unknown frame types are logged and
    skipped, never fatal.
- **Robustness.** A malformed line must not kill the pump: log it and continue,
  mirroring omp's own parse-error tolerance. The writer half closing (omp exit)
  drains the in-flight map with a terminal error.

## 3. Session and turn management

omp owns the session as a first-class, on-disk resource; `omprint` drives it.

- **Start a turn.** After the ready handshake, send
  `{ id, type: "prompt", message, images? }`. omp acknowledges immediately
  (`{ command: "prompt", success: true, data?: { agentInvoked } }`); the turn's
  output then streams as `AgentSessionEvent`s ending in `agent_end`. A
  `data.agentInvoked: false` (or a later `prompt_result` with
  `agentInvoked: false`) means the prompt resolved locally without a model turn
  (e.g. a slash command) — that is its own completion signal, with no `agent_end`.
- **Resume a conversation.** omp persists each session as a JSONL file under
  `~/.omp/agent/sessions/`. `omprint` stores that **session id / session-file
  path** on the conversation row when first seen, and resumes by spawning omp
  against it (the omp CLI resume affordance — `omp -c` / `omp -r` / a
  `switch_session` command pointed at the stored path) and then issuing the next
  `prompt`. Resume is deterministic: the session is identified by the stored row,
  never inferred.
- **Capturing the session id.** Read the current session via
  `{ type: "get_state" }`, whose response carries `sessionId` and `sessionFile`
  (plus `model`, `thinkingLevel`, `isStreaming`, `contextUsage`, …). `omprint`
  captures the id on first sight, persists it on the conversation row, and
  broadcasts a `session-created` event (the same hook the loop's title/host UI
  relies on).
- **Steering and follow-ups.** While a turn is streaming, a new `prompt` must
  carry `streamingBehavior: "steer"` (interrupt path) or `"followUp"` (post-turn
  path) — omp rejects a bare prompt during streaming. omprint exposes these for
  manual chat (a user typing while the agent works); the autonomous loop runs one
  prompt per turn and does not need them. Queue/interrupt behavior is tunable via
  `set_steering_mode` / `set_follow_up_mode` / `set_interrupt_mode`.

## 4. Event format and the event consumer

Because omp is the only backend, **omprint consumes omp's native
`AgentSessionEvent`s directly** — there is no unification step and no `provider`
discriminator. The event types omprint cares about:

| omp event | omprint use |
|---|---|
| `agent_start` / `agent_end` | turn boundaries; `agent_end` fires the completion hook |
| `turn_start` / `turn_end` | inner turn boundaries within a run |
| `message_start` / `message_update` / `message_end` | assistant output; `message_update.assistantMessageEvent` carries streaming **text / thinking / toolcall deltas** |
| `tool_execution_start` / `tool_execution_update` / `tool_execution_end` | tool calls and their results, for the live transcript |
| `auto_compaction_start` / `auto_compaction_end` | context compaction notices |
| `auto_retry_start` / `auto_retry_end` | omp's own retry signalling |
| `ttsr_triggered`, `todo_reminder`, `todo_auto_clear` | workflow hints |
| `subagent_lifecycle` / `subagent_progress` / `subagent_event` | sub-agent activity (gated by `set_subagent_subscription`) |
| `extension_error` | extension-runner failures (log, non-fatal) |

The shared event consumer iterates this stream and, per event:

- forwards **thinking/text deltas** (`message_update.assistantMessageEvent`) to
  the live UI and the thinking accumulator,
- **broadcasts the event to subscribed WebSocket clients** so the UI streams live,
- updates the **context-usage tracker** from `get_state`'s `contextUsage`
  (`{ tokens, contextWindow, percent }`) and any usage in terminal events,
- captures the **session id** on first sight (via `get_state`) and fires the
  session-created hook,
- on `agent_end`, fires the **`onComplete` lifecycle hook** — the seam the
  orchestration loop plugs into for completion and chaining (see
  [`orchestration-loop.md`](./orchestration-loop.md)).

A local-only prompt (`agentInvoked: false`) completes without `agent_end`; the
consumer treats that response/`prompt_result` as the terminal signal for that
prompt.

## 5. Transcript persistence

The transcript is the canonical record of every conversation and lives in
`omprint`'s embedded [`hiqlite`](https://github.com/sebadob/hiqlite) database.

- **omp-native, single source of truth.** Since there is one backend, the
  transcript schema stores omp's event shapes directly — **no compatibility
  layer, no `provider` column, no remapping to a neutral vocabulary.** As events
  stream, `omprint` appends them to the conversation's message rows (idempotent on
  a stable per-event id, with a monotonic `seq` per session) so a mid-turn reload
  of the conversation returns the live history.
- **omp's JSONL is private scratch.** omp also writes its own session JSONL under
  `~/.omp/agent/sessions/`. That on-disk file is what omp uses to **resume**;
  `omprint`'s database is the authoritative record the UI and history loads read
  from. The two are kept consistent: the database mirrors the live event stream;
  omp's JSONL is consulted only via the resume path (re-spawn against the stored
  session file).
- **History load.** Reading a conversation's transcript is a database read of its
  message rows. `{ type: "get_messages" }` is available to reconcile against omp's
  view of the session when needed (e.g. after a crash-resume), but steady-state
  rendering is served from the database.

## 6. Abort handling

Stopping a running turn is a two-part operation because the work runs
out-of-process in omp:

1. **RPC abort.** Send `{ id, type: "abort" }` (or `abort_and_prompt` to abort
   and immediately re-prompt) on the conversation's stdin. omp halts the current
   turn and acknowledges. `abort_bash` / `abort_retry` exist for the narrower
   cases of a running `bash` command or an in-flight auto-retry.
2. **PTY signal / teardown.** If omp does not stop (or the whole run is being
   killed), the pty layer escalates: SIGINT to the process, then teardown as in
   §1.

As the orchestration loop requires, a user **Stop** writes the linked agent-run
row to `failed` **synchronously, before** issuing the abort, so the completion
handler sees `failed` when the stream ends and does **not** chain (see
[`orchestration-loop.md`](./orchestration-loop.md), "completion is
database-driven"). Active turns are tracked in an in-memory map keyed by
conversation/session id so an abort can find the right subprocess.

## 7. Credentials and authentication

omp resolves model-provider credentials itself; `omprint`'s job is to hand the
spawned subprocess the right environment and let omp's own auth resolution take
over. There is **no per-provider credential registry inside omprint** — there is
one integration (omp), and omp fans out to its forty-plus model providers.

- **Per-user isolation via the agent dir.** omp keeps settings, credentials, and
  caches under `~/.omp/agent/` (credentials in `agent.db`). `PI_CODING_AGENT_DIR`
  relocates that base. `omprint` spawns each user's omp with a **per-user agent
  dir** (e.g. `~/.config/omprint/users/<userId>/omp/`) so one user's stored
  credentials never leak into another's turn — the same isolation posture the old
  harness specs achieved with `CODEX_HOME`/XDG pinning.
- **Environment-variable auth.** omp reads provider credentials from environment
  variables when no stored credential exists — e.g. `ANTHROPIC_OAUTH_TOKEN` then
  `ANTHROPIC_API_KEY` for Anthropic, `OPENAI_API_KEY` for OpenAI,
  `OPENAI_CODEX_OAUTH_TOKEN` for Codex, `GEMINI_API_KEY` for Google, and so on
  (full table in omp's providers docs). `omprint` injects the acting user's
  configured tokens into the spawn environment and **strips inherited global keys**
  it does not intend to forward, so the per-user credential wins over anything in
  the server's own `process.env`.
- **OAuth `/login` over RPC.** OAuth-backed providers (Anthropic, GitHub Copilot,
  Cursor, xAI-OAuth, the Gemini/Codex subscription paths, …) are normally attached
  via omp's login flow. The RPC surface exposes
  `{ type: "get_login_providers" }` and `{ type: "login", providerId }`; the login
  flow emits an `open_url` extension-UI request the host surfaces to the user.
  `omprint` drives this to let a user connect a subscription credential from the
  web UI without a terminal.
- **Where credentials come from per agent.** Which provider/model each agent role
  runs on, and where the user's tokens are stored, is configured by the
  model-customization extra
  ([`prompt-and-model-customization.md`](../extra/prompt-and-model-customization.md))
  and the app-auth extra
  ([`auth-and-multi-user.md`](../extra/auth-and-multi-user.md)). Core can hardcode
  a single env-supplied credential.

A turn that reaches omp with no usable credential surfaces omp's own
"run `/login` or set the provider env var" error; `omprint` maps that to a typed
**`OmpCredentialsMissing`** condition so the route layer renders a
"Connect provider" affordance (HTTP 403) instead of a 500.

## 8. Model and configuration

All per-turn configuration is expressed through omp's CLI options at spawn time
and its RPC commands mid-session:

- **Model selection.** `{ type: "set_model", provider, modelId }` selects the
  active model; `{ type: "get_available_models" }` lists what the current
  credentials unlock; `cycle_model` rotates the active role's configured models.
  omp's **roles** (`default`, `smol` for cheap sub-agent fan-out, `slow` for deep
  reasoning, `plan` for plan mode, `commit`) can also be pinned at launch
  (`--smol`, `--slow`, `--plan`). `omprint` maps each agent role's
  `(provider, model)` setting onto `set_model` (or the launch flag) — **the model
  is always explicit, never defaulted** (the deterministic-model rule, owned by
  [`prompt-and-model-customization.md`](../extra/prompt-and-model-customization.md)).
- **Reasoning effort.** `{ type: "set_thinking_level", level }` sets omp's
  thinking level (`off|minimal|low|medium|high|xhigh`); `cycle_thinking_level`
  rotates it. This is omp's analogue of the per-agent "effort" dimension.
- **System prompt.** The task-doc context block (`buildContextPrompt`, see
  [`task-and-workspace.md`](./task-and-workspace.md)) and any per-agent prompt
  override are injected via omp's system-prompt CLI option at spawn. `get_state`
  echoes the active `systemPrompt` for verification.
- **Permissions.** The autonomous loop runs agents non-interactively, so omp is
  spawned in a non-prompting posture (auto-approve edits/commands) appropriate to
  the worktree sandbox — the agent writes files and runs commands without parking
  on a human. Permission gating, when a host wants it, routes through the
  extension-UI sub-protocol (§9). Effective default for orchestrated runs:
  full-auto inside the task worktree.
- **Tool disables.** Agents that must stay in one observable conversation disable
  sub-agent fan-out — the implementation and yolo agents forbid the
  Agent/sub-agent tool (see [`execution-loop.md`](./execution-loop.md) and
  [`yolo-mode.md`](../extra/yolo-mode.md)); planning and refinement *allow* it.
  Tool disables are passed as omp CLI options at spawn.
- **Todos.** `{ type: "set_todos", phases }` can pre-seed a plan's checklist into
  omp's in-session todo state before the first prompt; `get_state.todoPhases`
  reads it back.

## 9. Feature support

Because omp is the sole, known backend, its features are **static knowledge**,
not a runtime capability matrix. Everything the old harness contract gated behind
`ProviderCapabilities` flags is simply available, via omp's RPC surface:

- **Streaming thinking + text deltas.** `message_update.assistantMessageEvent`
  carries incremental text/thinking/toolcall deltas — the live thinking widget
  and streaming transcript read these directly. (No `supportsThinkingDelta` gate.)
- **Mid-turn human gate (questions / permissions).** omp's extension-UI
  sub-protocol surfaces `select` / `confirm` / `input` / `editor` requests as
  `extension_ui_request` frames the host answers with `extension_ui_response`.
  This is the mechanism for "ask the user a question" and for any
  permission-prompt that a non-full-auto posture would raise. `omprint` parks the
  request, broadcasts an awaiting-answer event to the UI, and replies when the
  user answers. (Replaces the harness `AskUserQuestion`/`canUseTool` machinery.)
- **Context-usage breakdown.** `get_state.contextUsage`
  (`{ tokens, contextWindow, percent }`) and per-turn usage feed the live context
  meter. (No `supportsContextUsageBreakdown` gate.)
- **MCP servers.** omp ships its own tool surface and MCP layer (read, search,
  debugger, LSP, subprocesses, GitHub). MCP configuration is omp's; `omprint` does
  not re-wire it. (No `supportsMcpServers` gate.)
- **Images.** `prompt` / `steer` / `follow_up` accept an `images?: ImageContent[]`
  field, so image attachments ride along on the message. (No `supportsImages`
  gate.)
- **Host-owned tools & URIs.** Via `set_host_tools` and `set_host_uri_schemes`,
  `omprint` can expose its own callbacks (`host_tool_call` / `host_uri_request`)
  to the agent over the same channel — a forward-looking extension point (§11).

## 10. Integration with the orchestration loop

The orchestration loop ([`orchestration-loop.md`](./orchestration-loop.md))
drives omp through this integration, not through any abstraction:

- `startAgentRun(taskId, agentType)` resolves the effective cwd and the agent's
  `(provider, model, thinking-level, system prompt, tool disables)`, **spawns omp
  in RPC mode** with those, waits for `ready`, and sends the agent's prompt.
- The event consumer (§4) **broadcasts** each `AgentSessionEvent` to WebSocket
  subscribers and **persists** it (§5).
- On `agent_end` the consumer fires the **completion hook**, which marks the
  agent-run row `completed` (unless already `failed` from a Stop) and **chains**
  the next agent per the task flags. The four signalling actions
  (`complete-plan` / `complete-workflow` / `block-workflow` / `complete-pr`) are
  run by the agents inside their omp turn (the agent has shell access to the
  `omprint` bin) and flip task flags the handler reads after the turn ends.
- **Abort** (§6) writes `failed` synchronously then aborts, so chaining is
  suppressed.

The loop stays dumb: it never parses omp's prose, only reads task flags and the
agent-run status. omp's role is to *do the turn*; the integration's role is to
*run, observe, persist, and signal*.

## 11. Future extension points

The single-backend binding leaves deliberate room to deepen the omp integration
without re-introducing an abstraction:

- **Host-owned tools (`set_host_tools` / `host_tool_call`).** Expose
  omprint-native capabilities (e.g. a task-flag setter, a structured "ask the
  human" tool, project metadata lookups) to the agent as first-class omp tools,
  replacing shell-script signalling with typed tool calls.
- **Host-owned URI schemes (`set_host_uri_schemes` / `host_uri_request`).** Back
  virtual files the agent reads/writes through omprint — e.g. `task://`,
  `review://`, or `artifact://` schemes resolved against omprint's database and
  archive.
- **Sub-agent observability.** `set_subagent_subscription` + the `subagent_*`
  event stream can surface omp sub-agent activity in the omprint UI (live
  sub-agent trees), useful for the planning and refinement agents that fan out.
- **Session branching / forking.** omp's `branch` / `switch_session` / `handoff`
  commands enable forking a conversation from any prior message — a natural fit
  for "retry from here" or "fork this plan" workflows.
- **Richer model routing.** omp's per-role models, fallback chains, and
  round-robin credentials could be surfaced as omprint configuration for more
  resilient multi-key, multi-provider operation.

## What to build

- [ ] A pty-backed RPC client that spawns `omp --mode rpc` via `portable-pty`,
      waits for the `ready` frame, and pumps NDJSON frames both ways on a `tokio`
      worker.
- [ ] Command correlation (`id → oneshot`) with the documented edge cases
      (immediate prompt ack, unknown-id responses, `parse` errors, malformed-line
      tolerance).
- [ ] Subprocess lifecycle: spawn-per-conversation with per-user agent dir,
      graceful shutdown (close stdin), forced teardown (SIGINT→SIGTERM→SIGKILL),
      and crash → synthetic-error-result recovery.
- [ ] Session/turn driving: `prompt` to start, session-id capture via `get_state`,
      deterministic resume against the stored omp session file, and
      steer/follow-up for manual chat.
- [ ] An event consumer over native `AgentSessionEvent`s that broadcasts live,
      persists the transcript (omp-native, single source of truth), tracks context
      usage, captures the session id, and fires the `onComplete` hook on
      `agent_end`.
- [ ] Abort: RPC `abort` + synchronous agent-run `failed` write + pty escalation,
      with an in-memory active-session map.
- [ ] Credential handling: per-user agent dir, env-injected provider tokens with
      global-key stripping, the OAuth `/login` RPC flow, and a typed
      missing-credentials condition mapped to HTTP 403.
- [ ] Configuration: `set_model` / role selection (explicit model, never
      defaulted), `set_thinking_level`, system-prompt injection, full-auto
      permission posture, and tool disables per agent role.
- [ ] The extension-UI sub-protocol handler for mid-turn questions/permissions
      (`extension_ui_request` → broadcast → `extension_ui_response`).

## Boundaries (not in this spec)

- Which provider/model each agent uses, the deterministic-model rule, per-agent
  prompt overrides, and where per-user tokens are stored →
  [`prompt-and-model-customization.md`](../extra/prompt-and-model-customization.md).
- App-level auth (who may use omprint), distinct from omp's model-provider
  credentials → [`auth-and-multi-user.md`](../extra/auth-and-multi-user.md).
- How a finished turn drives the next agent (chaining, flags, the iteration cap) →
  [`orchestration-loop.md`](./orchestration-loop.md).
- Chat-only conveniences (slash commands, attachments, voice, the context-usage
  meter) → [`chat-ux.md`](../extra/chat-ux.md).
- An example omp configuration file → [`omp.config.example.yml`](../omp.config.example.yml).
