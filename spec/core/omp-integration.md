# Core — The direct `omp` integration

> **⚠️`omprint` ONLY ⚠️:** Rust convention requires functions and `let` bindings
> use `snake_case` as a naming convention; In all places where `camelCase`
> occurs (referring to the typescript `reference/` implementation of `bottega`),
> substitute for `snake_case` as appropriate; `PascalCase` is used for `trait`s,
> `struct`s, `enum`s, etc.
> 
> **Note:** The `omprint` Rust codebase at `src/` now provides implementations
> for many of the features described in this spec. Prefer citations to `src/`
> over `reference/` wherever equivalents exist.

This is the seam that makes the pipeline work. `omprint` runs all coding-agent
turns through [`omp` (`oh-my-pi`)][0] — there is exactly one harness, and this
spec describes exactly how that integration works.

## Why it is core

The orchestration loop and the agents are useless without something that
actually runs a coding-agent turn. For `omprint`, that thing is `omp` in RPC
mode, spawned as a subprocess via [`portable-pty`][1]. The loop never concerns
itself with multiple backends, credential stores for different providers, or a
capability matrix normalizing across SDKs — there is only `omp`.

One hard rule makes the seam real: **everything above this interface interacts
with `omp`'s native RPC protocol directly.** There is no abstraction layer, no
unified vocabulary normalizing across providers, and no provider registry.

## The `omp` subprocess

`omprint` spawns `omp --mode rpc` via `portable-pty`. Each agent turn is a fresh `omp`
session (or a resume of a prior session). The pty gives `omprint` `STDIN`/`STDOUT`
control, `pid` for audit, and `SIGKILL` for abort.

### Spawn command

```
omp --mode rpc
```

### Environment variables

| Variable | Purpose |
|---|---|
| `OMP_MODELS_YML` | Inline or file-path to the `models.yml` content (see models.yml passthrough below) |
| `OMPRINT_ARCHIVE_ROOT` | The task archive path, for agent context resolution |
| `PATH` | Inherited, so `omp` and `git` are available to the agent |

Additional environment variables from `omprint`'s configuration are passed
through as needed (e.g., proxy settings).

### Working directory (`cwd`)

Set to the **effective working directory** of the task — the worktree path if
the worktree exists, otherwise the repo path (with `subproject_path` appended
for monorepos). See [`task-and-workspace.md`](./task-and-workspace.md).

## The message protocol

`omp` communicates over `STDIO` using a **JSON-lines** protocol. Each line is a
self-contained RPC event with a `type` discriminator. This is the single wire
format — there is no normalization layer.

### Turn start

`omprint` writes the turn input (see Per-turn input below) as a JSON object to
`STDIN`, followed by a newline:

```json
{
  "type": "start",
  "session_id": null,
  "prompt": "...",
  "cwd": "/path/to/worktree",
  "model": "anthropic/claude-sonnet-4-20250514",
  "effort": "high",
  "permission_mode": "none",
  "disallowed_tools": [],
  "models_config": "..."
}
```

### Streaming events

`omp` writes JSON-lines to `STDOUT` as the turn progresses. Each line is a
discrete event:

| Event type | Description |
|---|---|
| `session_start` | The `omp` session has begun; carries `session_id` |
| `text` | Text delta from the assistant |
| `text_chunk` | Streaming text delta chunk |
| `tool_use` | The agent is invoking a tool |
| `tool_result` | The result of a tool invocation |
| `thinking` | Thinking/reasoning content |
| `thinking_chunk` | Streaming thinking delta |
| `context_usage` | Context usage breakdown |
| `error` | An error occurred |
| `done` | The turn is complete; carries the final result |

> **Rust implementation:** `src/omp/protocol.rs` defines all RPC event types
> (`OmpRpcEvent`, `TurnInput`, `ResumeInput`).

### Turn resume

To resume a prior session, `omprint` sends a resume message with the stored
`session_id`:

```json
{
  "type": "resume",
  "session_id": "<stored-session-id>",
  "messages": [...]
}
```

### Turn abort

`omprint` kills the pty subprocess with `SIGKILL`. There is no graceful
shutdown message — the subprocess is destroyed.

### Detecting turn completion

The `done` event signals that the turn has completed. `omprint` detects this
by monitoring the `STDOUT` stream for the `done` event type. When the
subprocess exits its `STDOUT` stream ends, which also signals completion
(if the `done` event was already received).

### Namespace convention for `omp` sub-agents

`omp` supports sub-agents for different roles [`plan`, `impl`, `review`, `pr`].
These map to `omprint`'s agent types as follows:

| `omprint` agent type | `omp` sub-agent | Purpose |
|---|---|---|
| `planification` | `plan` | Planning the work |
| `implementation` | `impl` | Implementing changes |
| `review` | `review` | Reviewing the implementation |
| `pr` | `pr` | Opening and managing the pull request |
| `refinement` | `plan` | Polishing between review and PR |
| `yolo` | `impl` | Single-agent mode |

`omprint` sets the `OMP_AGENT_TYPE` environment variable (or passes it in the
RPC start message) to inform `omp` which sub-agent configuration to use.

## Per-turn input

At turn start, `omprint` sends the following fields to `omp`:

| Field | Source | Description |
|---|---|---|
| `cwd` | Task worktree or repo path | The working directory for the agent |
| `model` | User settings or stored conversation row | The model identifier (e.g., `anthropic/claude-sonnet-4-20250514`). **Always explicit** — never defaulted or inferred. Resolved from `loadAgentModelSettings` on start, or from the stored conversation row on resume |
| `effort` | User settings or stored conversation row | Reasoning effort level |
| `prompt` | Composed agent prompt | The rendered agent message (see [`prompt-and-model-customization.md`](../extra/prompt-and-model-customization.md)) |
| `custom_system_prompt` | Task context | The task-doc context block (optional) |
| `permission_mode` | Server configuration | `none`, `approve`, or `reject` — controls whether `omp` auto-approves tool calls |
| `disallowed_tools` | Server configuration | Tools the agent is not allowed to use this turn |
| `models_config` | User's stored `models.yml` content | The raw YAML content from the settings UI, passed directly to `omp` |

**The `model` field is load-bearing.** It is resolved deterministically:
- On **start**: from the user's per-agent model settings (`loadAgentModelSettings`)
- On **resume**: from the stored `(model, omp_session_id)` on the conversation row

A turn never runs on a defaulted or inferred model. If the model cannot be
resolved, the turn fails immediately with a clear error.

## The streaming runtime

The core async (`tokio`) loop that drives each turn:

1. **Spawn** `omp --mode rpc` via `portable-pty`
2. **Write** the turn input (the start or resume message) to `STDIN`
3. **Iterate** `omp`'s `STDOUT` line-by-line, parsing each JSON event
4. **Broadcast** each event to subscribed WebSocket clients (live UI)
5. **Persist** the transcript to `hiqlite` as events stream (single source of truth)
6. **Capture** the `omp` session ID on the first `session_start` event
7. **Fire** the `on_complete` lifecycle hook when the stream ends or the `done` event is received

```rust
// Pseudocode for the streaming loop
let mut pty = PortablePty::spawn("omp --mode rpc", cwd, env)?;
let (mut stdin, mut stdout) = pty.split();

stdin.write_all(turn_input).await?;
stdin.flush().await?;

let mut session_id: Option<String> = None;
let mut transcript = Vec::new();
let mut lines = BufReader::new(stdout).lines();

while let Some(line) = lines.next_line().await? {
    let event: OmpRpcEvent = serde_json::from_str(&line)?;

    // Capture session ID on first sight
    if session_id.is_none() {
        if let OmpRpcEvent::SessionStart { id } = &event {
            session_id = Some(id.clone());
            persist_session_id(id).await;
        }
    }

    // Broadcast to WebSocket clients
    broadcast_event(&event).await;

    // Persist to hiqlite
    persist_event(&event).await;

    // Check for completion
    if matches!(event, OmpRpcEvent::Done { .. }) {
        break;
    }
}

// Fire the lifecycle hook
on_complete(session_id, transcript).await;
```

This is the seam that the orchestration loop plugs into — `startAgentRun` and
the completion handler wire in here. See [`orchestration-loop.md`](./orchestration-loop.md).

## Transcript persistence — the single source of truth

The transcript is the canonical record of every conversation, stored in
`hiqlite`, not in SDK files on disk.

### Tables

Two tables store the transcript:

- **`messages`** — one row per transcript entry, idempotent on `uuid`, with a
  monotonic `seq` per `(project_key, session_id)`.
- **`session_summaries`** — a folded summary sidecar per session.

### Write path

Events are **mirrored** into the `messages` table as they stream. Each event
is assigned:
- A stable `uuid` (derived from `omp`'s event id, or synthesized deterministically)
- A monotonic `seq` within the session
- The `project_key` (derived from the conversation's working directory)
- The `session_id` from `omp`

### Read path

`load_transcript(project_key, session_id)` reads from the `messages` table and
returns the ordered transcript as deserialized event objects. Used for:
- Loading history in the chat UI
- Assembling context for resume

## Session management

Three operations: start, resume, abort.

### Start

1. Resolve `(model, effort)` from the user's settings
2. Spawn a fresh `omp --mode rpc` subprocess via `portable-pty`
3. Write the turn input (start message) to `STDIN`
4. Capture the `session_id` from the first `session_start` event
5. Persist `(model, omp_session_id)` to the conversation row
6. Enter the streaming loop

### Resume

1. Read stored `(model, omp_session_id)` from the conversation row
2. Spawn a fresh `omp --mode rpc` subprocess via `portable-pty`
3. Write the resume message (with the stored `session_id` and any new messages)
   to `STDIN`
4. Enter the streaming loop

### Abort

1. Kill the pty subprocess with `SIGKILL`
2. Mark the linked `task_agent_runs` row as `failed` **synchronously**
   (so the completion handler won't chain — see [`orchestration-loop.md`](./orchestration-loop.md))

### Active session tracking

Active sessions are tracked in-memory in a `HashMap<SessionId, PtyHandle>` for
abort targeting. This map is maintained by the streaming runtime and cleaned up
when the session ends or is aborted.

## models.yml passthrough

`omprint` does not manage provider credentials directly. Instead:

1. **Users manage `models.yml` entries** through the settings UI — a textarea
   where they can write or paste YAML content defining provider backends and
   their API keys.
2. **Content is stored** in `omprint`'s database/configuration, scoped per user
   (or globally, depending on deployment).
3. **Injected on spawn.** The stored `models.yml` content is passed to the
   `omp --mode rpc` subprocess via the `OMP_MODELS_YML` environment variable (or an
   equivalent configuration mechanism).
4. **No `ProviderCredentialStore`.** Credential management delegates entirely
   to `omp`'s existing infrastructure — `models.yml` for API keys, `ENV` vars
   for provider auth.

This replaces the old `ProviderRegistration` concept. See
[`extra/harnesses/omp.md`](../extra/harnesses/omp.md) for the concrete
integration mechanism.

## Model listing

`models.yml` serves two purposes: provider configuration (API keys, base URLs)
and model lists for providers that lack built-in model listing (e.g., custom
providers like `llama.cpp`, `ollama`). The `LlmProvider` trait
exposes `get_models_list` (`src/providers/mod.rs:15`) which returns `Vec<String>`
of available model identifiers for the current provider configuration.

### The `omp models --json` command

`omp models --json` returns a structured JSON response describing all
configured providers and their available models:

```json
{
  "models": [
    {
      "provider": "anthropic",
      "models": ["claude-sonnet-4-20250514", "claude-3.5-haiku", "claude-opus-4"],
      "default": "claude-sonnet-4-20250514"
    },
    {
      "provider": "openai",
      "models": ["gpt-4o", "gpt-4o-mini"],
      "default": "gpt-4o"
    },
    {
      "provider": "bedrock",
      "models": ["us.anthropic.claude-sonnet-4-20250514-v1:0"],
      "default": "us.anthropic.claude-sonnet-4-20250514-v1:0"
    }
  ]
}
```

### Per-provider resolution

- **Providers with their own listing API** (Anthropic, OpenAI, etc.): `omp`
  queries the upstream API for the available model IDs. These are returned
  dynamically from the provider's API endpoint.
- **Built-in providers with local model lists** (`bedrock`, `vertex`, etc.): `omp`
  maintains the known model IDs internally. These providers ship with their own
  model lists and do not rely on user configuration.
- **Custom providers** (`llama.cpp`, `ollama`, etc.): The user must declare
  available models explicitly in `models.yml`, since these providers have no
  standard model listing mechanism.
- **Fallback:** If no models are configured for a provider, `omp` returns
  a single `"default"` entry as a placeholder.

### Usage in `omprint`

The model listing is surfaced in the settings UI to let users select which
model an agent type uses. The flow is:

1. User opens per-agent model settings in the UI
2. `omprint` calls `registry::get_models_for_config` (`src/providers/registry.rs:86`)
   which resolves the provider and calls `get_models_list()`
3. The returned model list populates a dropdown/selector in the settings UI
4. The user's selection is stored as the `model` field in the per-agent config,
   passed through to `omp` at turn start as the `model` field in the RPC message

## Orphan recovery

On `omprint` restart:

1. **Kill orphan subprocesses.** Any `omp` subprocess still alive from a prior
   run is killed (discovered via stored `pid` or process group).
2. **Sweep orphan runs.** Any `task_agent_runs` row still in `running` status
   is swept to `failed`, and the linked conversation is closed. This prevents
   the UI from showing stuck-in-progress states.

## Reference map

| Concern | Rust (implemented) | Legacy reference |
|---|---|---|
| PTY spawn and lifecycle | `src/omp/mod.rs` (`spawn_omp`, `OmpSession`) | `reference/server/services/providers/anthropic/index.ts` |
| Streaming reader loop | `src/omp/mod.rs` (`spawn_reader`) | `reference/server/services/conversation/runStreamingLoop.ts` |
| RPC protocol types | `src/omp/protocol.rs` | — |
| WebSocket broadcast | Not yet implemented | `reference/server/websocket/broadcast.ts` |
| Transcript persistence | `src/omp/transcript.rs` (`persist_event`, `load_transcript`) | `reference/server/services/sqliteSessionStore.ts`, `reference/server/database/init.sql` |
| Active session management | `src/omp/session.rs` (`start_session`, `resume_session`, `abort_session`) | `reference/server/services/conversation/sessionControl.ts` |
| `omp` RPC documentation | — | [https://omp.sh/docs](https://omp.sh/docs) |
| `models.yml` format | — | [https://omp.sh/docs/custom-models](https://omp.sh/docs/custom-models) |
| Model listing (`omp models --json`) | `src/providers/omp_provider.rs` (`get_models_list`), `src/providers/opencode_provider.rs` (`get_models_list`), `src/providers/registry.rs` (`get_models_for_config`) | — |

**FIXME:** Partially addressed. PTY spawn, RPC protocol, reader loop, transcript
persistence, and session management now point at `src/omp/`. WebSocket broadcast
remains to be implemented.

## What to build

- [x] `omp` subprocess spawning via `portable-pty` → `src/omp/mod.rs`
- [x] RPC message write/read loop → `src/omp/mod.rs` (`spawn_reader`), `src/omp/protocol.rs`
- [ ] Full streaming runtime with WebSocket broadcast (reader loop exists; broadcast TBD)
- [x] Transcript persistence to `hiqlite` → `src/omp/transcript.rs`
- [x] `load_transcript` → `src/omp/transcript.rs`
- [x] Session management: start, resume, abort → `src/omp/session.rs`
- [x] `models.yml` passthrough via `TurnInput.models_config` → `src/omp/mod.rs`, `src/omp/protocol.rs`
- [x] Orphan recovery on startup → `src/orchestration/recovery.rs`
- [x] Model listing (`get_models_list`) → `src/providers/omp_provider.rs`, `src/providers/opencode_provider.rs`, `src/providers/registry.rs`

## Boundaries (not in this spec)

- Concrete subprocess lifecycle, event mapping details, and capabilities →
  [`extra/harnesses/omp.md`](../extra/harnesses/omp.md).
- Which model an agent uses and where per-user settings come from →
  [`prompt-and-model-customization.md`](../extra/prompt-and-model-customization.md).
- How a finished turn drives the next agent →
  [`orchestration-loop.md`](./orchestration-loop.md).
- Chat-only conveniences (slash commands, attachments, voice, title generation,
  the context-usage meter) → [`chat-ux.md`](../extra/chat-ux.md).

[0]: https://omp.sh
[1]: https://github.com/wezterm/wezterm/tree/main/pty
