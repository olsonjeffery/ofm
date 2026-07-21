# OpenCode provider implementation patterns

> **⚠️`ofm` ONLY ⚠️:** Rust convention requires functions and `let` bindings
> use `snake_case` as a naming convention; In all places where `camelCase`
> occurs (referring to the typescript `reference/` implementation of `bottega`),
> substitute for `snake_case` as appropriate; `PascalCase` is used for `trait`s,
> `struct`s, `enum`s, etc

## What it is

The OpenCode provider is the built-in coding harness for `ofm`. It is
implemented as `OpenCodeSdkProvider` in
`src/providers/opencode_sdk_provider.rs` and communicates with a local
`opencode serve` subprocess through the `opencode_sdk` submodule. The SDK
handles subprocess lifecycle, HTTP communication, and typed event
subscriptions. The provider implements the `LlmProvider` trait (defined in
`src/providers/mod.rs`) and maps SDK typed events (`GlobalEvent` / `Event`) to
`ProviderEvent` for compatibility with the streaming runtime.

This file documents the `OpenCodeSdkProvider` implementation, which serves as
the sole built-in provider harness for `ofm`.

## Subprocess lifecycle

### Per-user server pool

The reference implementation (see
`spec/reference/server/services/openCodeServerPool.ts`) pools opencode
servers **per user** — every conversation for a user shares one
`opencode serve` subprocess. This keeps the session_id stored on the
conversation row valid across Stop Agent / turn completion / new
turns, because the session lives on the shared server.

`ofm` mirrors this design with [`OpenCodeServerPool`] in
`src/opencode_sdk/pool.rs`. The pool is a process-wide singleton
(accessed via `OpenCodeServerPool::instance()`) keyed by `user_id`. Each
entry owns a running `opencode serve` subprocess and an `OpencodeClient`
handle. Callers go through `get_or_spawn(user_id, harness_config,
config_root)` — never spawn directly.

```rust
let client = OpenCodeServerPool::instance()
    .get_or_spawn(user_id, &harness_config, &config_root)
    .await?;
// client: OpencodeClient — cheap `Arc` clone, used for every
// session-scoped operation. The underlying `OpenCodeServer` is owned by
// the pool and persists across Stop Agent / turn completion.
```

### Spawning (pooled)

`OpenCodeServerPool::get_or_spawn` checks the pool first:

- **Existing entry** — bump `last_used_at` and return the cached client.
- **Stale entry** — wait for the in-flight shutdown to finish, then
  spawn fresh.
- **No entry** — call `opencode_sdk::create_opencode()` which spawns
  `opencode serve` on a random port with a temp config directory and a
  generated password, then waits for `GET /global/health` to return 200
  before returning.

Concurrent `get_or_spawn` calls for the same user coalesce via a
`pending` map keyed by `user_id` — only one spawn happens at a time.

### Spawning (transient)

One-shot operations (`get_models_list`, `one_shot_prompt`, title
generation) spawn a fresh server+client pair via
`OpenCodeSdkProvider::spawn_transient`, use it, and shut it down within
the same method call. Transient servers do **NOT** participate in the
pool — they're discarded on return.

### Lifecycle phases

1. **`LlmProvider::start()`** — Acquires a pooled client for the user
   via `get_or_spawn`. Stores it on the provider. The server itself stays
   in the pool.
2. **`LlmProvider::start_turn` / `resume_turn`** — Use the borrowed
   client to create sessions, send prompts, and subscribe to events.
3. **Stop Agent (`POST /agent_runs/reset`)** — Calls
   `provider.abort_turn()` (cancel SSE listener + best-effort
   `session.abort`). The provider is **NOT** removed from
   `active_sessions` and the pooled opencode subprocess is **NOT**
   killed, so `resume_turn` on the same session_id continues to work.
4. **Turn completion (`Done` event)** —
   `orchestration::completion_handler` marks the run `completed` but
   does **NOT** call `provider.shutdown()` — the server stays alive in
   the pool so the user can resume the conversation.
5. **`LlmProvider::shutdown()`** — Noop for the pooled provider: cancels
   the in-flight event stream and drops the borrowed client handle. The
   underlying `opencode serve` subprocess stays alive in the pool.
6. **Idle reaping** — A background `tokio::spawn` task started in
   `src/main.rs` wakes every 60 seconds and SIGKILLs pool entries whose
   `last_used_at` is older than 15 minutes. Matches the reference's
   `DEFAULT_IDLE_TIMEOUT_MS = 15 * 60 * 1000`.
7. **Credential invalidation** — `OpenCodeServerPool::invalidate(user_id)`
   removes the user's entry; `Drop` on the owned `OpenCodeServer` kills
   the child. The next `get_or_spawn` spawns a fresh server. Called by
   settings routes when the user's provider config changes (TODO:
   wire this in once settings writes are verified end-to-end).
8. **Process-exit cleanup** — `src/main.rs` installs `SIGTERM` /
   `SIGINT` handlers that call
   `OpenCodeServerPool::instance().shutdown_all()`. Every entry is
   dropped, and each `OpenCodeServer`'s `Drop` implementation kills the
   child subprocess. Matches the reference's `pool.shutdownAll()` on
   signal.

### Health check

Health check polling is handled internally by
`opencode_sdk::create_opencode()`. The SDK polls `GET /global/health` with
`Authorization: Bearer {password}` every 200ms for up to 20 attempts.

### Config via `opencode.json`

The provider builds its server configuration as a `serde_json::Value` through
`build_server_config()`:

```rust
fn build_server_config(&self) -> serde_json::Value {
    let mut base = serde_json::json!({
        "provider": {},
        "permission": {
            "edit": "allow",
            "bash": "allow",
            "webfetch": "allow",
            "doom_loop": "allow",
            "external_directory": "allow"
        }
    });
    if let Ok(snippet) = serde_json::from_str::<serde_json::Value>(&self.provider_snippet) {
        deep_merge(&mut base, &snippet);
    }
    base
}
```

User-provided provider configuration (loaded from the config directory) is
merged on top via `deep_merge` in the same file. The resulting `Value` is
passed as `ServerOptions.config` — the SDK writes it to the temp directory's
`opencode.json` and sets the `OPENCODE_CONFIG` environment variable.

The `deep_merge` function recursively overlays an object onto a base,
preserving keys from both:

```rust
fn deep_merge(base: &mut serde_json::Value, overlay: &serde_json::Value) {
    match (base, overlay) {
        (Value::Object(base_map), Value::Object(overlay_map)) => {
            for (key, val) in overlay_map {
                if base_map.contains_key(key) {
                    deep_merge(&mut base_map[key], val);
                } else {
                    base_map.insert(key.clone(), val.clone());
                }
            }
        }
        (base, overlay) => *base = overlay.clone(),
    }
}
```

## SDK event streaming protocol

The SDK provides typed event streams via `OpencodeClient::event::subscribe()`.
The provider spawns a `tokio::spawn` task that iterates over the stream and
maps each `GlobalEvent` to a `ProviderEvent` sent through an `mpsc::Sender`:

```rust
let event_stream = client.event.subscribe().await?;
let cancellation = event_stream.cancellation_handle();
// stored in self.event_cancellation for later abort

let (tx, rx) = mpsc::channel(1024);
tokio::spawn(async move {
    let mut stream = event_stream;
    while let Some(result) = stream.next().await {
        match result {
            Ok(global) => {
                if let Some(provider_event) = map_sdk_event_to_provider_event(&global, &session_id) {
                    if tx.send(provider_event).await.is_err() { break; }
                }
            }
            Err(e) => {
                let _ = tx.send(ProviderEvent::Error { error: e.to_string() }).await;
                break;
            }
        }
    }
});
```

### Turn start flow

1. `client.session.create(&input.prompt).await` — Creates a new session on the
   server. The SDK sends `POST /session` and returns the session ID.
2. `client.event.subscribe().await` — Subscribes to the typed event stream
   (backed by the SDK's SSE connection to `GET /event`). The provider spawns
   a reader task (see above).

   > **Ordering matters.** The subscription is established *before* the
   > prompt is dispatched. The opencode server emits events synchronously
   > as the prompt is queued — if the SSE connection is opened after
   > `prompt_async` returns, the early `message.part.updated` /
   > `session.status` events fire before the reader task exists and are
   > lost, leaving the conversation inert.
3. `client.session.prompt_async(&session_id, &body).await` — Sends the prompt
   with model selection. The SDK sends
   `POST /session/{id}/prompt_async` with a JSON body:
   ```json
   {
     "model": {"provider_id": "anthropic", "model_id": "claude-sonnet-4-20250514"},
     "parts": [{"type": "text", "text": "..."}]
   }
   ```

### Event mapping

The `map_sdk_event_to_provider_event` function maps SDK typed events to
`ProviderEvent`:

| SDK `Event` variant | `ProviderEvent` variant | Notes |
|---|---|---|
| `MessagePartUpdated { part: Text }` | `TextChunk { delta }` | Streaming text delta |
| `MessagePartUpdated { part: Reasoning }` | `Thinking { thinking }` | Reasoning content |
| `MessagePartUpdated { part: Tool { state: Running } }` | `ToolUse { tool_name, tool_use_id, input }` | Tool invocation |
| `MessagePartUpdated { part: Tool { state: Completed } }` | `ToolResult { tool_use_id, result }` | Tool output |
| `MessagePartUpdated { part: Tool { state: Error } }` | `Error { error }` | Tool execution error |
| `SessionStatus { status: "error" }` | `Error { error }` | Session-level error |
| `SessionError` | `Error { error }` | SDK session error |
| `SessionStatus { status: "idle" }` | `Done` | Turn complete — normal termination |
| `SessionIdle` | `Done` | Alternative termination signal |
| `ServerConnected` | `Ready` | Server initialized |

Events that don't match (unknown types, `SessionStatus` with other status
values, events for non-matching session IDs, `ToolState::Pending`) are
silently dropped — the stream continues.

### Turn resume

**Supported.** `resume_turn` extracts the last user message from the
transcript (via `input.messages`) and sends a new prompt to the existing
session using `client.session.prompt_async()`. As with `start_turn`, the
event subscription is established *before* the prompt is dispatched so
early events are not lost:

```rust
async fn resume_turn(&self, input: ResumeInput) -> Result<mpsc::Receiver<ProviderEvent>, ProviderError> {
    let client = self.client.lock().unwrap().clone().ok_or(ProviderError::NotStarted)?;
    let session_id = input.session_id;
    let prompt = extract_last_user_message(&input.messages).unwrap_or("continue");

    let rx = self.subscribe_and_spawn(&client, &session_id).await?;
    let body = self.build_prompt_body(&prompt, &self.config.model.as_deref().unwrap_or("default"));
    client.session.prompt_async(&session_id, &body).await?;
    Ok(rx)
}
```

### Turn abort

Abort uses a two-part approach:
1. Cancel the event stream via `cancellation.cancel()` (from the SDK's
   `EventStreamCancellation` handle) to stop the reader task immediately.
2. Call `client.session.abort(&session_id).await` to tell the server to stop
   processing. Errors are silently ignored (best-effort cancellation).

### One-shot prompts

`one_shot_prompt` uses the SDK's `opencode_sdk::conversation::one_shot()`
function, which creates a transient session, sends the prompt, collects the
full response, and cleans up:

```rust
let config = opencode_sdk::conversation::OneShotConfig {
    model: model.to_string(),
    ..Default::default()
};
let result = opencode_sdk::conversation::one_shot(&client, prompt, &config).await?;
```

The transient server is created and shut down within the method.

## Session lifecycle

### Start

1. Resolve `(model, effort)` from the user's settings
2. If no server is running, spawn via `opencode_sdk::create_opencode()` in
   `start()`
3. `client.session.create()` — creates session via `POST /session`
4. `client.event.subscribe()` + `tokio::spawn` reader task on the event
   stream (established **before** the prompt is sent so early events
   are not lost)
5. `client.session.prompt_async()` — sends prompt via `POST /session/{id}/prompt_async`

### Abort

1. `cancellation.cancel()` — stops the SDK event stream reader
2. `client.session.abort(&session_id)` — best-effort server-side cancellation
3. The agent run row is marked `failed` synchronously (see
   [`orchestration-loop.md`](../../core/orchestration-loop.md))

### Shutdown

`cancellation.cancel()` (if running) + drop the borrowed `OpencodeClient`
handle. The pooled provider's `LlmProvider::shutdown()` is a **noop**
with respect to subprocesses — it does NOT kill the `opencode serve`
process, which stays alive in `OpenCodeServerPool`. The temp directory
with `opencode.json` is cleaned up when the `OpenCodeServer`'s
`TempDir` is dropped (only happens when the pool drops the entry on
idle reap, invalidation, or `shutdown_all`).

### Process-exit cleanup

The `ofm` process installs Ctrl-C and SIGTERM handlers in `src/main.rs`.
On shutdown, it calls
`OpenCodeServerPool::instance().shutdown_all().await`, which drops every
pool entry. Each entry's `OpenCodeServer::Drop` implementation kills the
child subprocess via `child.kill()` + `child.wait()`. Without this, the
child processes outlive `ofm` and pile up as orphaned `opencode serve`
instances on the host. `state.active_sessions` is also cleared so any
in-flight broadcast tasks see their channels close; the providers
themselves hold no subprocesses (the pool owns them).

## Credential delegation

Credentials are managed through the `agent_harness_configs` / provider config
system. The OpenCode provider:

1. Loads provider configuration from the config directory via
   `ProviderConfigDir::load_provider_config()`
2. Stores the raw JSON snippet in `self.provider_snippet`
3. Merges the user's provider snippet into a base config via `build_server_config()`
4. Passes the merged config to the SDK via `ServerOptions.config`

The OpenCode binary handles all credential resolution from its own config file.

## Capabilities

Since the OpenCode provider sends events through the `LlmProvider` trait, it
inherits the same event loop infrastructure. Notable characteristics:

- **SDK-backed transport** — the `opencode_sdk` submodule manages subprocess
  lifecycle, HTTP communication, and typed event subscriptions
- **Models list** — fetched from `client.config.providers()` (backed by
  `GET /config/providers`)
- **Turn resume** — fully supported by extracting the last user message from
  the transcript and re-prompting the same session

## What to build

- [x] SDK-backed subprocess spawn of `opencode serve` with temp config and
      auto-generated password → `src/providers/opencode_sdk_provider.rs`
      (`start_server_common`, `start`)
- [x] Config building and deep-merge → `src/providers/opencode_sdk_provider.rs`
      (`build_server_config`, `deep_merge`), `src/providers/config.rs`
      (`ProviderConfigDir`)
- [x] Session creation via SDK (`client.session.create`,
      `client.session.prompt_async`) → `src/providers/opencode_sdk_provider.rs`
      (`start_turn`)
- [x] SDK event stream reader mapping typed `GlobalEvent` → `ProviderEvent` →
      `src/providers/opencode_sdk_provider.rs` (`subscribe_and_spawn`,
      `map_sdk_event_to_provider_event`)
- [x] Abort via SDK event cancellation + `client.session.abort` →
      `src/providers/opencode_sdk_provider.rs` (`abort_turn`)
- [x] One-shot prompt via `opencode_sdk::conversation::one_shot` →
      `src/providers/opencode_sdk_provider.rs` (`one_shot_prompt`)
- [x] Models list fetch via `client.config.providers` →
      `src/providers/opencode_sdk_provider.rs` (`get_models_list`)
- [x] Turn resume via `client.session.prompt_async` + event subscription →
      `src/providers/opencode_sdk_provider.rs` (`resume_turn`)

## Reference map

| Concern | Rust (implemented) |
|---|---|
| `LlmProvider` trait | `src/providers/mod.rs` |
| `OpenCodeSdkProvider` impl | `src/providers/opencode_sdk_provider.rs` |
| `opencode_sdk` submodule | `src/providers/opencode_sdk/` |
| Config loading (provider config dir) | `src/providers/config.rs` |
| Provider registry | `src/providers/registry.rs` |
| Provider event types (shared) | `src/providers/types.rs` |

## Boundaries (not in this spec)

- The orchestration loop that drives turns →
  [`../../core/orchestration-loop.md`](../../core/orchestration-loop.md).
- Which model an agent uses and how per-user settings are resolved →
  [`../../extra/prompt-and-model-customization.md`](../../extra/prompt-and-model-customization.md).
