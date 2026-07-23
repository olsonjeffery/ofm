# OpenCode Rust SDK

## Purpose

The `opencode_sdk` module provides an idiomatic Rust surface for the
`@opencode-ai/sdk@1.15.5` npm package. It enables Rust consumers — primarily
the ofm crate itself — to interact with the opencode HTTP API and manage
opencode server processes.

This is a clean-room implementation that is **not** influenced by the existing
`LlmProvider` trait. It lives in its own root module at `src/opencode_sdk/`.

## Module Structure

```
src/opencode_sdk/
├── mod.rs          # Module wiring, re-exports, SdkError, create_opencode() factory
├── types.rs        # All wire-format types (Event, Message, Part, Session, etc.)
├── server.rs       # OpenCodeServer lifecycle (spawn, health check, shutdown)
├── client.rs       # OpencodeClient with SessionApi, EventApi, ConfigApi
└── conversation.rs # High-level conversation patterns
```

### Dependencies

Uses only existing crate dependencies: `serde`, `serde_json`, `reqwest`,
`tokio`, `futures-util`, `base64`, `uuid`, `tempfile`, `thiserror`, `bytes`.

No new external dependencies are introduced.

## Type Reference

### Event Types

All SSE events use a `GlobalEvent` wrapper matching the opencode wire format:

```rust
pub struct GlobalEvent {
    pub directory: String,
    pub payload: Event,
}
```

The `Event` enum uses serde adjacently tagged format with `type` as the tag
and `properties` as the content — matching the wire format:

```json
{"type": "session.idle", "properties": {"sessionID": "s1"}}
```

#### Event Variants (32 total)

| Variant | Type String | Properties |
|---|---|---|
| `MessagePartUpdated` | `message.part.updated` | `MessagePartUpdatedData { part: Part, delta: Option<String> }` |
| `MessageUpdated` | `message.updated` | `MessageUpdatedData { info: AssistantMessage, parts: Option<Vec<Part>> }` |
| `MessageRemoved` | `message.removed` | `MessageRemovedData { session_id, message_id }` |
| `MessagePartRemoved` | `message.part.removed` | `MessagePartRemovedData { session_id, message_id, part_id }` |
| `SessionStatus` | `session.status` | `SessionStatusData { session_id, status: SessionStatusValue { status_type } }` |
| `SessionIdle` | `session.idle` | `SessionIdData { session_id }` |
| `SessionCreated` | `session.created` | `SessionCreatedData { session_id, session: Session }` |
| `SessionUpdated` | `session.updated` | `SessionUpdatedData { session_id, session: Session }` |
| `SessionDeleted` | `session.deleted` | `SessionIdData { session_id }` |
| `SessionError` | `session.error` | `SessionErrorData { session_id, error }` |
| `SessionCompacted` | `session.compacted` | `SessionIdData { session_id }` |
| `SessionDiff` | `session.diff` | `SessionDiffData { session_id, diff: Value }` |
| `ServerConnected` | `server.connected` | `ServerConnectedData { version?, config? }` |
| `ServerInstanceDisposed` | `server.instance.disposed` | `Value` |
| `FileEdited` | `file.edited` | `Value` |
| `TodoUpdated` | `todo.updated` | `Value` |
| `CommandExecuted` | `command.executed` | `Value` |
| `FileWatcherUpdated` | `file_watcher.updated` | `Value` |
| `VcsBranchUpdated` | `vcs.branch.updated` | `Value` |
| `PtyCreated` | `pty.created` | `PtyEventData { pty_id, session_id, cols?, rows? }` |
| `PtyUpdated` | `pty.updated` | `PtyOutputData { pty_id, session_id, data }` |
| `PtyExited` | `pty.exited` | `PtyExitData { pty_id, session_id, code }` |
| `PtyDeleted` | `pty.deleted` | `PtyIdData { pty_id, session_id }` |
| `InstallationUpdated` | `installation.updated` | `Value` |
| `InstallationUpdateAvailable` | `installation.update_available` | `Value` |
| `LspClientDiagnostics` | `lsp.client_diagnostics` | `Value` |
| `LspUpdated` | `lsp.updated` | `Value` |
| `PermissionUpdated` | `permission.updated` | `PermissionData { permission_id, session_id, permission_type, description? }` |
| `PermissionReplied` | `permission.replied` | `PermissionReplyData { permission_id, session_id, approved }` |
| `TuiPromptAppend` | `tui.prompt_append` | `Value` |
| `TuiCommandExecute` | `tui.command_execute` | `Value` |
| `TuiToastShow` | `tui.toast_show` | `TuiToastData { message, toast_type, duration? }` |

### Part Types

Parts use internally tagged serde with `type` as discriminator:

```rust
pub enum Part {
    Text(TextPart),
    Reasoning(ReasoningPart),
    Tool(ToolPart),
    File(FilePart),
    StepStart(StepStartPart),
    StepFinish(StepFinishPart),
    Snapshot(SnapshotPart),
    Patch(PatchPart),
    Agent(AgentPart),
    Retry(RetryPart),
    Compaction(CompactionPart),
    Subtask,
}
```

### Message Types

Messages use internally tagged serde with `role` as discriminator:

```rust
pub enum Message {
    User(UserMessage),
    Assistant(AssistantMessage),
}
```

### ToolState

```rust
pub enum ToolState {
    Pending(ToolStatePending),
    Running(ToolStateRunning),
    Completed(ToolStateCompleted),
    Error(ToolStateError),
}
```

### Other Types

- `Session` — `{ id, directory, title?, model?, agent?, created?, updated? }`
- `Provider` — `{ id, name, source, env, key?, options?, models }`
- `PartInput` — `{ Text(TextPartInput) | File(FilePartInput) | Agent(AgentPartInput) | Subtask }`
- `PromptBody` — `{ message_id?, model?, agent?, no_reply?, system?, tools?, parts }`
- `TokenUsage` — `{ input, output, reasoning?, cache? }`
- `PromptResponse` — `{ info: AssistantMessage, parts: Vec<Part> }`

## API Reference

### OpencodeClient

Factory: `create_opencode_client(base_url: &str, password: Option<&str>) -> OpencodeClient`

Namespaced sub-clients:
- `client.session` — `SessionApi`
- `client.event` — `EventApi`
- `client.config` — `ConfigApi`

#### SessionApi

| Method | HTTP | Description |
|---|---|---|
| `create(title)` | `POST /session` | Create a new session |
| `get(id)` | `GET /session/{id}` | Get session by ID |
| `list()` | `GET /session` | List all sessions |
| `delete(id)` | `DELETE /session/{id}` | Delete a session |
| `prompt(id, body)` | `POST /session/{id}/message` | Synchronous prompt (blocking, returns full response) |
| `prompt_async(id, body)` | `POST /session/{id}/prompt_async` | Fire-and-forget prompt (returns 204, events via SSE) |
| `abort(id)` | `POST /session/{id}/abort` | Abort active generation |
| `messages(id)` | `GET /session/{id}/message` | Get message history |

#### EventApi

| Method | HTTP | Description |
|---|---|---|
| `subscribe()` | `GET /event` | Subscribe to SSE event stream |

Returns an `EventStream` implementing `futures::Stream<Item = Result<GlobalEvent, SdkError>>`.

#### ConfigApi

| Method | HTTP | Description |
|---|---|---|
| `providers()` | `GET /config/providers` | List configured providers |

### OpenCodeServer

Created via `create_opencode_server(options: ServerOptions)`.

```rust
pub struct ServerOptions {
    pub hostname: String,        // default: "127.0.0.1"
    pub port: u16,               // 0 = pick free port
    pub timeout: Duration,       // health check timeout (default: 10s)
    pub working_dir: Option<PathBuf>,
    pub config: Option<Value>,   // merged into opencode.json
    pub password: Option<String>, // auto-generated if None
}
```

Methods:
- `url() -> String` — server base URL
- `password() -> Option<&str>` — server auth password
- `port() -> u16` — listening port
- `hostname() -> &str` — bind hostname
- `shutdown() -> Result<bool>` — kill process tree, verify port released

The `Drop` implementation kills the child process as a safety net.

### SdkError

```rust
pub enum SdkError {
    Http(reqwest::Error),
    Io(std::io::Error),
    Protocol(String),
    Timeout,
}
```

### Convenience Factory

```rust
pub async fn create_opencode(
    options: ServerOptions,
) -> Result<(OpencodeClient, OpenCodeServer), SdkError>
```

Starts a server and returns a pre-configured client pointing at it.

## Conversation Patterns

### Phase-based Conversation (PhaseConversation)

Structured agent workflow with phases. Owns a server instance.

```rust
let mut conversation = PhaseConversation::start(server_opts, &phase_config).await?;
let mut events = conversation.run_phase("Analyze the code", "analysis").await?;
while let Some(event) = events.next().await {
    // process events
}
conversation.close().await?;
```

The `PhaseEventStream` filters the SSE stream for events matching the
session and terminates on `session.idle` or `session.error`.

### One-shot Conversation

Synchronous, no SSE. Sends a prompt and returns the response text:

```rust
let text = one_shot(&client, "What is Rust?", &OneShotConfig::default()).await?;
```

Flow: create session → `session.prompt()` → extract text → delete session.

### Unstructured Conversation

Free-form, user-driven conversation with multiple turns:

```rust
let conv = UnstructuredConversation::start(&client).await?;
let mut stream = conv.send_message("Hello").await?;
// consume events...
let messages = conv.messages().await?;
conv.abort().await?;
```

## Server Lifecycle

### When to use `create_opencode_server` directly

When you need fine-grained control over the server, such as:
- Custom port allocation
- Custom configuration
- Manual lifecycle management

### When to use `create_opencode` factory

When you need both server and client and want a single call to set up.

### When to use conversation patterns

When you want a higher-level abstraction that manages session lifecycle,
event filtering, and cleanup automatically.

## Testing

### Unit tests

Located as `#[cfg(test)]` modules in each source file:

- `types.rs` — JSON roundtrip for every Event variant (38), every Part variant
  (12), Message, PromptBody, Session, Provider, ToolState, TokenUsage.
- `client.rs` — URL construction, request body serialization, SSE line parsing.
- `server.rs` — Port picking, config generation, auth header format.
- `conversation.rs` — Event matching, terminal event detection, config defaults.
- `mod.rs` — N/A (re-exports only).

### Integration tests

Located in `tests/opencode_sdk_integration_test.rs`. Guarded by
`has_binary("opencode")` — skipped when the binary is not on PATH.

Test scenarios:

| # | Test | Scenario |
|---|---|---|
| 1 | `test_server_lifecycle` | Start server → shutdown |
| 2 | `test_server_shutdown_releases_port` | Start → shutdown → port probe |
| 3 | `test_create_opencode_and_client` | Factory → client check |
| 4 | `test_session_lifecycle` | Create → get → list → delete |
| 5 | `test_config_providers` | Config API call |
| 6 | `test_one_shot_pattern` | One-shot prompt |
| 7 | `test_abort_session` | Abort active session |
| 8 | `test_concurrent_sessions` | Multiple parallel sessions |
| 9 | `test_error_on_invalid_session` | Error handling |
| 10 | `test_prompt_async_and_abort` | Async prompt + abort |

### Running tests

```bash
# Unit tests only
cargo test --lib -- opencode_sdk

# Integration tests
cargo test --test opencode_sdk_integration_test

# All SDK tests
cargo test -- opencode_sdk
```

## Design Decisions

1. **One-shot uses `session.prompt()` (synchronous)** — per user decision.
   Returns the full `PromptResponse` immediately without SSE.

2. **Multi-turn uses `session.prompt_async()` (fire-and-forget)** — events
   arrive via SSE subscription. The `EventStream` wrapper handles parsing
   and filtering.

3. **Server assumes `opencode` on PATH** — no download/install logic.
   Returns `SdkError::Protocol("opencode binary not found in PATH")` if
   missing.

4. **Auth uses Basic auth** — generated password per server instance,
   matching the existing pattern in the ofm codebase.

5. **Clean-room implementation** — completely independent of the existing
   `LlmProvider` trait.

6. **Process cleanup** — `shutdown()` kills the process group, kills
   grandchild processes via `ps --ppid`, and probes the port to confirm
   the process is dead. The `Drop` implementation provides a safety net
   for unexpected drops.
