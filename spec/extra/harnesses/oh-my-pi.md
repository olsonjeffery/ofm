# oh-my-pi-specific implementation patterns

> **⚠️`ofm` ONLY ⚠️:** Rust convention requires functions and `let` bindings
> use `snake_case` as a naming convention; In all places where `camelCase`
> occurs (referring to the typescript `reference/` implementation of `bottega`),
> substitute for `snake_case` as appropriate; `PascalCase` is used for `trait`s,
> `struct`s, `enum`s, etc

## What it is

This file describes how `ofm` actually implements the integration with
`oh-my-pi` over `STDIO` via `portable-pty`: subprocess lifecycle, event mapping from
`oh-my-pi`'s RPC protocol to `ofm`'s internal message types, transcript mirroring
into `hiqlite`, credential delegation, and capabilities.

## Subprocess lifecycle

### Spawning

`ofm` uses [`portable-pty`][0] to create a pseudoterminal, fork `omp --mode rpc`
(see `src/providers/oh_my_pi/mod.rs` → `spawn_oh_my_pi`), and obtain:

- `pid` — the subprocess process id (for audit and abort)
- `STDIN` writer — to send RPC messages
- `STDOUT` reader — to receive RPC events

### Per-turn lifecycle

Each agent turn gets a **fresh `oh-my-pi` subprocess** (spawning `omp --mode rpc`). The subprocess:

1. Is spawned at turn start
2. Receives the turn input (start/resume message) via `STDIN`
3. Streams events back via `STDOUT`
4. Exits (or is killed) when the turn ends

There is no persistent subprocess — one subprocess per turn keeps the lifecycle
simple and avoids state leakage between turns.

### Abortion

On abort:

1. `ofm` kills the pty subprocess with `SIGKILL`
2. The agent run row is written to `failed` **synchronously**
3. The completion handler sees the `failed` status and does not chain

```rust
fn abort(pid: u32) {
    // Send SIGKILL to the subprocess
    kill(pid, SIGKILL);
    // Mark the agent run as failed synchronously
    mark_agent_run_failed(task_agent_run_id);
}
```

### Cleanup

When a turn ends normally:

- The `oh-my-pi` subprocess exits on its own
- `ofm` closes its `STDIN` writer and `STDOUT` reader
- The pty handle is dropped, releasing system resources

## Event mapping

`oh-my-pi`'s native RPC events (JSON-lines on `STDOUT`) map directly to `ofm`'s
internal message types. There is no normalization across providers — these are
the types `ofm` uses:

### Internal message types

| Type | Description |
|---|---|
| `user` | A message from the user (the prompt) |
| `assistant` | Assistant text response |
| `assistant_thinking` | Assistant thinking/reasoning content |
| `tool_use` | Agent is invoking a tool |
| `tool_result` | Result of a tool invocation |
| `system` | System messages (errors, status) |
| `result` | Terminal result of the turn |
| `stream_delta` | Streaming delta for progressive updates |

### RPC event to internal type mapping

| `oh-my-pi` RPC event | Internal type | Notes |
|---|---|---|
| `session_start` | (internal, not persisted) | Captured for session management |
| `text` | `assistant` | Full assistant text turn |
| `text_chunk` | `stream_delta` | Streaming text start; carries `delta` |
| `tool_use` | `tool_use` | Carries `tool_name`, `input`, `tool_use_id` |
| `tool_result` | `tool_result` | Carries `tool_use_id`, `result` |
| `thinking` | `assistant_thinking` | Full thinking turn |
| `thinking_chunk` | `stream_delta` | Streaming thinking start with subtype |
| `context_usage` | (event broadcast, not persisted) | Fed to the context usage tracker |
| `error` | `system` (subtype: `'error'`) | Error messages from `oh-my-pi` |
| `done` | `result` | Terminal event; carries `model_usage` |

### Sequencing and IDs

- Each event receives a monotonic `seq` per `(project_key, session_id)`
- IDs are derived from `oh-my-pi`'s event IDs when present, or synthesized
  deterministically otherwise
- The `provider_session_id` is stamped on every message from the `session_start`
  event

### Error/unparseable events

If an RPC event line cannot be parsed, `ofm` emits a `system` message with
`subtype: 'unknown'` containing the raw line. The stream continues — an
unparseable event never crashes the turn.

## Transcript mirroring

Events are **mirrored** into `hiqlite` as they stream. This is the
explicit-mirror pattern — `oh-my-pi` has no built-in session store hook, so
`ofm` writes each event to the database itself.

### Write-through

```
on each parsed event:
  1. assign monotonic seq for (project_key, session_id)
  2. insert into messages table
  3. if first event: persist session_id on conversation row
```

### Idempotency

The `messages` table uses an **upsert on `uuid`** — the same event arriving
twice (on reconnect, for example) overwrites rather than duplicates.

### Monotonic `seq`

`seq` is computed as `max(seq) + 1` for the given `(project_key, session_id)`
pair. This ensures events are always in order regardless of write timing.

### `load_transcript`

Reads back from `hiqlite` for history display and resume context assembly.
See `src/services/transcript.rs`.

## Credential delegation

`ofm` does **not** store provider credentials. All credential management
delegates to `oh-my-pi`'s existing infrastructure:

- **API keys** are stored in `models.yml` entries, managed by the user through
  `oh-my-pi`'s configuration tooling or `ofm`'s settings UI (the `models.yml`
  textarea).
- **Environment variables** for provider auth (e.g., `ANTHROPIC_API_KEY`,
  `OPENAI_API_KEY`) are set in `oh-my-pi`'s environment, not in `ofm`'s.
- `ofm`'s role is limited to:
  1. Storing the user's `models.yml` content (as raw YAML text)
  2. Passing it to `omp` on spawn via `OMP_MODELS_YML` env var
  3. Letting `oh-my-pi` handle all credential resolution

There is no `ProviderCredentialStore`, no credential registry, and no per-provider
auth flow in `ofm`.

## Capabilities

`oh-my-pi` supports the following capabilities:

| Capability | Value | Notes |
|---|---|---|
| `supports_ask_user_question` | `true` | `oh-my-pi` supports asking the user for clarification |
| `supports_thinking_delta` | `true` | Streaming thinking/reasoning deltas |
| `supports_context_usage_breakdown` | `true` | Per-category context usage reporting |
| `supports_mcp_servers` | `true` | MCP server integration |
| `supports_images` | `true` | Image attachments in messages |

Verify these against [`oh-my-pi` documentation][1] for the current state.

> **NOTE regarding `ask` in RPC mode:** `oh-my-pi`'s `ask` tool is tightly coupled to the TUI and is unavailable in RPC mode; we need to build an ask tool and inject it into `oh-my-pi` on startup (we will probably need to build several tools for custom/tight-integration)

## What to build

- [x] `portable-pty` subprocess spawning for `oh-my-pi` (`omp --mode rpc`) with `pid`, `STDIN`
      writer, `STDOUT` reader → `src/providers/oh_my_pi/mod.rs` (`spawn_oh_my_pi`)
- [x] Per-turn subprocess lifecycle (fresh subprocess per turn, cleanup on end)
      → `src/providers/oh_my_pi/mod.rs` (`OhMyPiSession::start_turn`, `OhMyPiSession::resume_turn`,
      `Drop` impl kills child)
- [x] Abort: `SIGKILL` + synchronous `failed` write on agent run
      → `src/services/session.rs` (`abort_session`), `src/providers/oh_my_pi/mod.rs` (`Drop` impl)
- [x] Event parser: JSON-lines decoder mapping `oh-my-pi` RPC events to internal
      message types with the table above → `src/providers/types.rs` + `spawn_reader`
      in `src/providers/oh_my_pi/mod.rs`
- [x] Error recovery for unparseable events (emit warning, continue)
      → `src/providers/oh_my_pi/mod.rs` (`spawn_reader` handles parse errors gracefully)
- [x] Transcript mirror: write-through to `hiqlite` `messages` table, monotonic
      `seq` → `src/services/transcript.rs` (`persist_event`)
- [x] `load_transcript`: read from `hiqlite` ordered by `seq`
      → `src/services/transcript.rs`
- [x] `OMP_MODELS_YML` env var injection on subprocess spawn
      → `src/providers/oh_my_pi/mod.rs` (`spawn_oh_my_pi` passes env_vars via `cmd.env`)

See also the sibling [opencode.md](./opencode.md) harness spec for the OpenCode
provider, which uses HTTP+SSE instead of `portable-pty`.

## Reference map

| Concern | Rust (implemented) | Legacy reference |
|---|---|---|---|
| Subprocess spawn + lifecycle | `src/providers/oh_my_pi/mod.rs` | `reference/server/services/providers/anthropic/index.ts` |
| Event parsing + reader loop | `src/providers/oh_my_pi/mod.rs`, `src/providers/types.rs` | (pattern: `mapMessage.ts`) |
| Transcript mirror | `src/services/transcript.rs` | (pattern: `messageMirror.ts`) |
| Session store | `src/services/session.rs` | (pattern: `sqliteSessionStore.ts`) |
| `oh-my-pi` RPC docs | — | [https://omp.sh/docs](https://omp.sh/docs) |
| `models.yml` format | — | [https://omp.sh/docs/custom-models](https://omp.sh/docs/custom-models) |

## Boundaries (not in this spec)

- The orchestration loop that drives turns →
  [`../../core/orchestration-loop.md`](../../core/orchestration-loop.md).
- Which model an agent uses and how per-user settings are resolved →
  [`../../extra/prompt-and-model-customization.md`](../../extra/prompt-and-model-customization.md).

[0]: https://github.com/wezterm/wezterm/tree/main/pty
[1]: https://omp.sh/docs
