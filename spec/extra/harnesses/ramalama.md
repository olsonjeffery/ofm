# RamaLama provider implementation patterns

## What it is

The RamaLama provider is an on-demand, on-device SLM harness for `ofm`. When
`OFM_RAMALAMA_PHI4_MINI_ENABLED=true` is set and the `ramalama` CLI is on
`PATH`, a virtual built-in model config named **`ramalama-mini`** appears in
the Agent Settings dropdown; assigning it to an agent type routes that agent's
turns through a local `ramalama serve` (llama.cpp backend) instead of a
cloud/hosted provider.

It is implemented as `RamalamaProvider` in
`src/providers/ramalama_provider.rs` and implements the same `LlmProvider`
trait (defined in `src/providers/mod.rs`) as `OpenCodeSdkProvider`.

## Virtual config entry & sentinel UUID

`ramalama-mini` is **not** stored in `user_model_configs` and no provider
config file is written. Instead:

- The `GET /api/settings/config-body` handler injects a virtual
  `UserModelConfig` with sentinel id
  `00000000-0000-0000-0000-00000000dead` (`RLML_MINI_SENTINEL_ID` in
  `src/providers/mod.rs`) whenever the feature flag is enabled.
- `upsert_agent_models()` (in `src/services/settings.rs`) detects the sentinel
  and stores `harness: "ramalama"` with the sentinel as `provider_config_ref`,
  skipping `write_provider_config()`.
- The `loadConfigList()` JS (in `src/webapp/pages/settings.rs`) filters the
  sentinel out of the Model Configurations tab, so the entry only ever appears
  in the Agent Settings dropdown.
- `resolve_harness_config()` resolves a sentinel-backed config exactly like any
  other; `RamalamaProvider::new()` reads the model name from
  `HarnessConfig.model` (falling back to `phi4-mini`).

## Subprocess lifecycle

Unlike `OpenCodeSdkProvider`, the ramalama host serves a single
model/session at a time, so the provider is **per-conversation** and
**not pooled**:

1. `ensure_started()` probes `ramalama` on PATH, picks a free port via
   `rauthy::find_available_port()`, and spawns
   `ramalama serve --port <port> --name <container> <model-ref>` as a
   `tokio::process::Child` owned by the provider. The model is passed as a
   **positional argument** — `--name` is the container name, not the model —
   and short names like `phi4-mini` are resolved to
   `ollama://library/...:latest` by `resolve_model_ref()`. The container gets a
   port-derived name (`ofm-ramalama-<port>`) so it can be removed precisely.
   Stdout/stderr are piped to tracing readers. Health is polled at
   `http://127.0.0.1:<port>/v1/models` (llama.cpp returns 404 for the
   Ollama-style `/api/tags`), also aborting if the child exits prematurely;
   after readiness `/v1/models` is queried again to resolve the **served model
   id** (e.g. `library/phi4-mini`), which may differ from the user-entered
   short name. On success it logs
   `RamaLama server started at http://127.0.0.1:<port>/v1`.
2. `start_turn` builds an OpenAI-compatible provider snippet declaring a custom
   `@ai-sdk/openai-compatible` provider with `options.baseURL =
   http://127.0.0.1:<port>/v1` and a `models` map keyed by the served model id
   (this shape is required for opencode to route the prompt; a bare
   `apiKey`/`baseUrl` snippet makes `opencode serve` return 500). It merges the
   snippet into a base opencode server config and spawns a **transient**
   `opencode serve` via `opencode_sdk::create_opencode()`. The transient server
   + client persist on the provider so `resume_turn` can reuse the opencode
   `session_id`.
3. `one_shot_prompt` (conversation-title generation) reuses the running
   `ramalama serve` and spins up a throwaway transient server per call.
4. `shutdown()` kills + reaps the `ramalama serve` child, removes the named
   container (`docker rm -f <name>` with a `ramalama stop <name>` fallback),
   and drops the transient server (its `Drop` kills the opencode subprocess).
   `Drop` on the provider performs a belt-and-suspenders container removal +
   `start_kill()` on the ramalama child.

### Teardown rules

- The ramalama child is owned by exactly one `RamalamaProvider`; `shutdown()`
  is the authoritative kill + reap path.
- **Killing the CLI alone orphans the container.** The provider always assigns
  a known `--name` and removes the container by name on both `shutdown()` and
  the `ensure_started()` error path, so a failed start never leaves a running
  container behind.
- No `process_group(0)`: the child is killed precisely on its owned handle via
  `start_kill()`/`kill()`, never bulk-killed.

## `conversation_title` agent type

`ConversationTitle` is a micro-task agent type (not a workflow phase), so it is
deliberately **absent** from `resolve_agent_config_statuses()` — the same
reason `yolo` is absent. It appears as its own row in Agent Settings so users
can assign a config (potentially `ramalama-mini`) used specifically for
generating conversation titles.

Title generation in `src/orchestration/mod.rs` now performs a separate
`resolve_harness_config(&AgentType::ConversationTitle, ...)` lookup instead of
reusing the agent-run config; when none is configured, title generation is
skipped gracefully.

## Graceful degradation

- `ramalama` not on PATH → `start()`/`ensure_started()` returns a config error
  with a clear log message; agent runs fail cleanly.
- Health-check timeout / premature exit → the child is cleaned up and the
  error is surfaced.
