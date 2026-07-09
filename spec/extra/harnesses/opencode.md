# OpenCode provider implementation patterns

> **⚠️`ofm` ONLY ⚠️:** Rust convention requires functions and `let` bindings
> use `snake_case` as a naming convention; In all places where `camelCase`
> occurs (referring to the typescript `reference/` implementation of `bottega`),
> substitute for `snake_case` as appropriate; `PascalCase` is used for `trait`s,
> `struct`s, `enum`s, etc

## What it is

An alternative provider harness to the core `omp` integration. Where
`omp` communicates over `STDIO` via `portable-pty`, the OpenCode provider
communicates over **HTTP + SSE** by spawning a local `opencode serve` process.
It implements the same `LlmProvider` trait (defined in `src/providers/mod.rs`)
and maps OpenCode's native event types to `OmpRpcEvent` for compatibility with
the existing streaming runtime.

The core [`omp-integration.md`](../../core/omp-integration.md) spec defines the
integration contract; this file documents how the OpenCode provider implements it.

## Subprocess lifecycle

### Spawning

The provider spawns `opencode serve` as a child process via
`std::process::Command`. Unlike `omp`'s `portable-pty` approach, OpenCode uses
a standard OS subprocess because the communication is over HTTP, not `STDIO`:

```rust
let child = std::process::Command::new("opencode")
    .arg("serve")
    .arg("--port").arg(port.to_string())
    .arg("--hostname").arg(&hostname)
    .env("OPENCODE_CONFIG", temp_dir.path())
    .env("OPENCODE_SERVER_PASSWORD", &password)
    .stdout(std::process::Stdio::null())
    .stderr(std::process::Stdio::null())
    .spawn()?;
```

### Server lifecycle

The OpenCode server has a persistent lifecycle separate from individual turns:

1. **`start()`** — Spawns `opencode serve` on a free port with a temporary
   config directory and auto-generated server password. Waits for the health
   endpoint (`GET /global/health`) to return 200 before returning.
2. **Per-turn** — Uses the running server's base URL and password to create
   sessions and send prompts via HTTP.
3. **`shutdown()`** — Kills the child process and drops the temp directory.
4. **Transient mode** — For one-shot operations (e.g., `get_models_list`,
   `one_shot_prompt`), a temporary server is spawned, used, and killed
   automatically if no persistent server is running.

### Health check

```rust
async fn wait_for_health(client, base_url, password) {
    // Poll GET /global/health with Authorization: Bearer {password}
    // Every 200ms, up to 20 attempts (4 second total timeout)
}
```

### Config via `opencode.json`

The provider writes a merged `opencode.json` configuration to a temporary
directory. The base config disables telemetry and sets an empty provider map;
user-provided provider configuration is merged on top via `merge_configs` in
`src/providers/config.rs`:

```rust
let base_config = r#"{"providers":{},"telemetry":{"enabled":false}}"#;
let merged = merge_configs(base_config, &provider_cfg)?;
std::fs::write(temp_dir.path().join("opencode.json"), &merged)?;
```

The `OPENCODE_CONFIG` environment variable points the OpenCode binary at this
temporary directory. The `OPENCODE_SERVER_PASSWORD` env var secures the HTTP API.

## SSE streaming protocol

Unlike `omp`'s JSON-lines over `STDIO`, OpenCode uses **Server-Sent Events**
(SSE) over HTTP. The provider maps SSE `data:` lines to `OmpRpcEvent` via
`map_opencode_event_to_omp_event` in `src/providers/opencode_provider.rs`.

### Turn start flow

1. `POST /session` — Creates a new session. Returns `{"id": "<session-id>"}`.
2. `POST /session/{id}/prompt_async` — Sends the prompt with model selection:
   ```json
   {
     "model": "anthropic/claude-sonnet-4-20250514",
     "parts": [{"type": "text", "text": "..."}]
   }
   ```
3. `GET /event` — SSE stream that emits events as the turn progresses.
4. Read SSE lines until a `done`/`completed` event is received.

### Event mapping

| OpenCode SSE event | `OmpRpcEvent` variant | Notes |
|---|---|---|
| `message.updated` (role: `assistant`) | `TextChunk { delta }` | Streaming text delta; user messages are ignored |
| `tool_use` | `ToolUse { tool_name, tool_use_id, input }` | Carries tool name, ID, and input params |
| `tool_result` | `ToolResult { tool_use_id, result }` | Carries tool use ID and result string |
| `thinking` | `Thinking { thinking }` | Reasoning content |
| `error` | `Error { error }` | Error message from provider |
| `done` | `Done` | Turn complete — normal termination |
| `completed` | `Done` | Alternative termination signal |

Events not in this table (unknown types, `message.updated` with role `user`)
are silently dropped — the stream continues.

### SSE reader loop

The reader (`read_sse_to_completion`) processes SSE data using
`tokio_stream::StreamExt` on the HTTP response's byte stream. Lines are
buffered, split on newlines, and each line prefixed with `data: ` is parsed.
Parsed events are forwarded through an `mpsc::Sender` via `blocking_send`.
On `Done`, the reader returns immediately.

### Turn resume

**Not supported.** The `resume_turn` method returns
`ProviderError::Protocol("resume_turn not supported by OpenCodeProvider")`.

### Turn abort

`POST /session/current/abort` — Sends an abort request to the running session.
Errors are silently ignored (best-effort cancellation).

### One-shot prompts

The `one_shot_prompt` method creates a temporary session, sends the prompt,
collects the full SSE response into a string, then deletes the session
(`DELETE /session/{id}`). If no persistent server is running, it spawns a
transient server, uses it, and kills it.

## Session lifecycle

### Start

1. Resolve `(model, effort)` from the user's settings
2. If no server is running, spawn `opencode serve` via `start()`
3. `POST /session` to create a session
4. `POST /session/{id}/prompt_async` with the turn input
5. Enter the SSE reader loop on `GET /event`

### Abort

1. `POST /session/current/abort` — best-effort cancellation
2. The agent run row is marked `failed` synchronously (see
   [`orchestration-loop.md`](../../core/orchestration-loop.md))

### Shutdown

`POST /session/current/abort` (if running) + `kill` child process + wait for
exit. The temp directory with `opencode.json` is cleaned up when
`OpenCodeServer`'s `TempDir` is dropped.

## Credential delegation

Credentials are managed through the same `agent_harness_configs` / provider
config system used by `omp`. The OpenCode provider:

1. Loads provider configuration from the config directory via
   `ProviderConfigDir::load_provider_config()`
2. Merges the user's provider snippet into a base `opencode.json` config
3. Writes the merged config to a temp directory
4. Passes the config path via `OPENCODE_CONFIG` env var

The OpenCode binary handles all credential resolution from its own config file.

## Capabilities

Since the OpenCode provider sends events through the `LlmProvider` trait, it
inherits the same event loop infrastructure. Notable limitations:

- **No resume support** — each turn is a fresh session
- **SSE transport** — events stream over HTTP instead of `portable-pty` STDIO
- **Models list** — fetched from `GET /config/providers` endpoint

## What to build

- [x] Subprocess spawn of `opencode serve` with temp config and auto-generated
      password → `src/providers/opencode_provider.rs` (`spawn_transient_server`,
      `start()`)
- [x] Health check polling (`GET /global/health`) → `src/providers/opencode_provider.rs`
- [x] Config file merge and write (`opencode.json`) → `src/providers/config.rs`
      (`merge_configs`), `src/providers/opencode_provider.rs`
- [x] Session creation via HTTP (`POST /session`, `POST /session/{id}/prompt_async`)
      → `src/providers/opencode_provider.rs` (`start_turn`)
- [x] SSE reader loop mapping events to `OmpRpcEvent` → `src/providers/opencode_provider.rs`
      (`read_sse_to_completion`, `map_opencode_event_to_omp_event`)
- [x] Abort via HTTP (`POST /session/current/abort`) → `src/providers/opencode_provider.rs`
      (`abort_turn`)
- [x] One-shot prompt support (transient session + SSE collection) →
      `src/providers/opencode_provider.rs` (`one_shot_prompt`,
      `collect_response_via_sse`)
- [x] Models list fetch (`GET /config/providers`) → `src/providers/opencode_provider.rs`
      (`get_models_list`)

## Reference map

| Concern | Rust (implemented) |
|---|---|
| `LlmProvider` trait | `src/providers/mod.rs` |
| OpenCode provider impl | `src/providers/opencode_provider.rs` |
| Config merge / provider config | `src/providers/config.rs` |
| Provider registry | `src/providers/registry.rs` |
| RPC event types (shared) | `src/omp/protocol.rs` |

## Boundaries (not in this spec)

- The core integration contract — PTY vs HTTP, RPC protocol, streaming runtime,
  transcript persistence, session management →
  [`../../core/omp-integration.md`](../../core/omp-integration.md).
- The orchestration loop that drives turns →
  [`../../core/orchestration-loop.md`](../../core/orchestration-loop.md).
- Concrete `omp` harness patterns (subprocess lifecycle via `portable-pty`,
  event mapping, transcript mirroring, credential delegation) →
  [`./omp.md`](./omp.md).
- Which model an agent uses and how per-user settings are resolved →
  [`../../extra/prompt-and-model-customization.md`](../../extra/prompt-and-model-customization.md).
