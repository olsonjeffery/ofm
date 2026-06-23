# Extra — Prompt and model customization

Two independent customization layers sit on top of the core agents. Neither is
required; core ships fixed prompts and can hardcode a single model
(see [`../core/omp-integration.md`](../core/omp-integration.md)). This extra
makes both **configurable** — the *what an agent says* and the *what runs it*.

## What it adds

1. **Prompt overrides.** Every agent prompt is a markdown template with
   `{{variable}}` placeholders. Defaults ship with the app; a user (or operator)
   can drop a same-named file in an override directory to replace any one of them
   without touching code. A small template engine renders them per run.
2. **Per-user model/effort selection.** Each user stores a full
   `Record<AgentType, {provider, model, thinkingLevel}>` — which model-provider,
   which model, and what thinking level each of the six agent roles runs on. This
   selects the underlying model omp routes to, **not** the coding-agent backend
   (that is always `omp`); see Layer 2. Core resolves this at run start. The
   cardinal rule: **the model is always explicit and resolved deterministically**,
   never defaulted or inferred.

The two layers are orthogonal — you can take prompt overrides without per-user
models, or vice versa — but they share one seam: the agent's turn input. The
prompt layer decides the *message* the agent receives; the model layer decides
the `(provider, model, thinkingLevel)` triple `omprint` applies to the `omp`
subprocess (via `set_model` + `set_thinking_level`, or launch flags — see
[`../core/omp-integration.md`](../core/omp-integration.md)). Both are assembled
in `startAgentRun` (the function core's
[`orchestration-loop.md`](../core/orchestration-loop.md) defers to this spec for
steps 2 and 6).

---

## Layer 1 — Prompt overrides

### Prompts are markdown templates, not string literals

The core agents (planning, implementation, review, PR — plus the `refinement`
and `yolo` extras and the `pr-feedback` webhook variant) each have a default
prompt as a `.md` file under `server/constants/prompts/`, and the plan template
under `server/constants/templates/`. None of the agent logic embeds prompt text;
it loads a named template and renders it.

A **prompt definition registry** is the source of truth for what prompts exist:
each entry carries a `name`, a human `label`, a `kind` (`prompt` or `template`),
the on-disk `file`, and the **allowlisted variable set** that template may
reference. See the `PROMPT_DEFINITIONS` array in
[`../reference/server/services/promptRenderer.ts`](../reference/server/services/promptRenderer.ts).
The registry is what the settings UI lists and what variable-validation checks
against.

### The override lookup: default vs `~/.bottega/prompts/`

Resolution is two-tier and dead simple. For a prompt named `X`:

- The **default** lives at `server/constants/{prompts,templates}/X.md` (bundled
  with the app).
- An **override** may live at `<archiveRoot>/{prompts,templates}/X.md`, where
  `archiveRoot` is `$BOTTEGA_ARCHIVE_ROOT` or `~/.bottega` by default.

`loadPrompt(name)` returns the override file if it exists, otherwise the default
(`loadOverride` → `loadDefault`). That is the entire override mechanism — file
presence wins. `hasOverride`, `saveOverride` (creates the dir, writes, returns
mtime), and `deleteOverride` (revert to default) round out the CRUD; study
`getOverridesDir` / `loadPrompt` / `resolvePromptPath` in `promptRenderer.ts`.
Note this is a **single instance-wide override directory**, not per-user — the
override is an operator-level customization of agent behavior, distinct from the
per-user model layer below.

### The template engine and the variable contract

`render(template, vars)` does `{{var}}` substitution and — critically —
**throws on a missing variable** rather than rendering an empty string, so a
typo'd placeholder surfaces immediately instead of silently corrupting a prompt.
`extractVariables` and `findUnknownVariables` enforce the other direction: a
candidate override may only reference variables in that prompt's allowlist, so a
user can't introduce `{{nonexistent}}` that will blow up at run time. Templates
(`kind: 'template'`, e.g. the plan template) are read **verbatim by the agent**
and never go through `render()`, so `{{ }}` markers in them are literal text and
validation is skipped — see the guard in `findUnknownVariables`.

The variable set per prompt is small and stable: most carry `taskDocPath` and
`taskId`; planning adds `planTemplatePath`; the PR/YOLO prompts add
`prContextLine` and `prCreateOrVerifyBlock`; the feedback prompt adds `prUrl`
and `feedbackSection`. The exact lists are the `variables` arrays in the
registry.

### How `agentPrompts.ts` composes a per-agent message

[`../reference/server/constants/agentPrompts.ts`](../reference/server/constants/agentPrompts.ts)
is the bridge between "a template file" and "the message a turn receives." For
each agent it: pre-builds any **dynamic sections** in JS (loops/conditionals the
template engine can't express — e.g. `buildPrCreateOrVerifyBlock` choosing
"create a new PR" vs "verify the existing PR", or the webhook feedback section
quoting review comments), then calls `renderPrompt(name, vars)` to inject them.
The one cross-prompt reference worth noting: planning injects
`resolvePromptPath('plan-template')` as `planTemplatePath` so the rendered
planning prompt points the agent at the *active* plan template (override if one
exists, else default) by absolute path — see `generatePlanificationMessage`.

This rendered string is the agent's turn `prompt`. It is distinct from the
`customSystemPrompt` (the task-doc context block) — `startAgentRun` passes both
into the conversation. The tech/non-tech planning split
(`planification` vs `planification-nontechnical`) is selected here too, but the
*role logic* behind that choice belongs to
[`auth-and-multi-user.md`](./auth-and-multi-user.md).

---

## Layer 2 — Per-user model and thinking level

### One settings blob per user, six agents each

Each user owns one row in `user_agent_model_settings` (`user_id` PK,
`settings_json` TEXT) holding their full
`Record<AgentType, {provider, model, thinkingLevel}>` for the six agent types
with settings (`planification`, `implementation`, `refinement`, `review`, `pr`,
`yolo`). See the table in
[`../reference/server/database/init.sql`](../reference/server/database/init.sql)
and the type in
[`../reference/shared/types/agentModelSettings.ts`](../reference/shared/types/agentModelSettings.ts).
This **replaced** a single global `app_settings.agent_model_settings` blob — the
point of going per-user is that each user runs agents on a model-provider they
actually hold credentials for.

`provider`/`model` here name an **omp model provider and model** — Anthropic,
OpenAI, OpenAI Codex, Google, xAI, Groq, OpenRouter, and the rest of omp's
forty-plus providers — **not** a coding-agent backend. The coding agent is always
`omp`; this layer only selects which underlying model omp routes the turn to.

### The model namespace is omp's

omp owns the catalog of providers and models. `omprint` does **not** maintain its
own hardcoded per-provider model enums; it reads what the active credentials
unlock from omp at runtime:

- `{ type: "get_available_models" }` over the RPC channel returns the models the
  current omp agent dir can reach; the settings UI populates its dropdowns from
  this live list rather than a baked-in array. **Never hardcode a model id** that
  could drift — a stale id fails loud (`set_model` returns
  `success: false, error: "Model not found: provider/model"`).
- A setting is stored as `(provider, model)` matching omp's
  `set_model { provider, modelId }` shape, plus a `thinkingLevel` from omp's
  `off|minimal|low|medium|high|xhigh` scale (the analogue of the old per-provider
  "effort" dimension; omp normalizes it via `set_thinking_level`). A provider/model
  with no reasoning dimension simply pins `thinkingLevel: off`.

There is no capability matrix to consult — omp's features (streaming deltas,
context usage, MCP, images, the mid-turn question gate) are uniformly available
(see [`../core/omp-integration.md`](../core/omp-integration.md)); this spec only
picks the `(provider, model, thinkingLevel)`.

### Resolution at run start — `loadAgentModelSettings(userId)`

[`../reference/server/services/agentModelSettings.ts`](../reference/server/services/agentModelSettings.ts)
owns resolution. `loadAgentModelSettings(userId)` reads the row, parses the JSON,
and validates **every one of the six entries**; a missing row, unparseable JSON,
or any invalid entry throws `MissingUserAgentSettingsError` — it **never returns
a silent default**. `startAgentRun` then:

1. Resolves the acting user (fails if there is none — there's no user to resolve
   settings for).
2. Pulls `loadAgentModelSettings(userId)[agentType]` →
   `(provider, model, thinkingLevel)`.
3. **Validates credentials up front** — checks that the acting user has a usable
   omp credential for the selected `provider` (a stored token in their per-user
   omp agent dir, or an env-supplied key omprint will inject), re-throwing as
   `OmpCredentialsMissing` so the route layer can render a "Connect &lt;provider&gt;"
   prompt (HTTP 403) instead of a stacktrace — this is the 403 the core trigger
   surface mentions.
4. **Stamps `(provider, model, thinkingLevel)` onto the new `task_agent_runs` and
   `conversations` rows** before starting the turn, then applies them to the
   spawned `omp` subprocess via `set_model` / `set_thinking_level` (or launch
   flags).

See `agentRunner.ts` around the `loadAgentModelSettings` call and the
`conversationsDb.create(taskId, provider, model, thinkingLevel)` line. Stamping
the conversation row is what makes the model **deterministic on resume**.

### The deterministic-model rule

This is the load-bearing invariant: a turn **never runs on a defaulted or
inferred model**. On start, the model comes from the user's setting; on resume, it
comes from the stored conversation row, never re-derived. `resolveResumeModel`
re-resolves `(model, thinkingLevel)` from the **resuming** user's setting only
when that setting targets the *same* provider the conversation was created
with; any mismatch, an unseeded resuming user, a manual chat, or a programmatic
resume falls back to the row's stored values. The failure mode this prevents is
sending an unreachable model id to `set_model`; the rule is "fail loud, don't
guess." (omp itself can be configured with per-role fallback chains for resilience
— see [`../core/omp-integration.md`](../core/omp-integration.md) — but the
*selection* omprint stamps is always explicit.)

### Seeding and backfill

Because resolution fails loud, every user must be seeded **before** they can run
an agent. Two mechanisms:

- **Seed on first provider-connect.** After a successful credential write (or an
  omp `/login` that attaches a subscription credential),
  `ensureUserAgentModelSettings(userId)` (via the non-throwing
  `seedAgentSettingsAfterConnect` wrapper) seeds all six agents to a sensible
  default model for the connected provider — resolved from omp's live
  `get_available_models` for that provider, declining to seed if none resolves
  (guessing an id is forbidden). See `buildSeedSettings` /
  `defaultSettingForProvider`.
- **Backfill.** A one-shot migration replicates a historical global default into
  a per-user row for pre-existing users.

A **blocking first-login provider modal** is the UX that guarantees a seed
exists: a brand-new user with no connected provider can't dismiss it, so they
can't reach a state where `startAgentRun` would throw on an unseeded user.

### The settings UI and routes

`GET/PUT /api/user-agent-model-settings` reads and replaces this user's full
six-agent map; `GET .../connected-providers` returns which providers this user
has credentials for. The `GET` returns `{ needsSeeding: true }` (not an error)
when the user is unseeded, which the UI uses to show the connect-provider state.
See
[`../reference/server/routes/userAgentModelSettings.ts`](../reference/server/routes/userAgentModelSettings.ts).
The settings tab (a "Agent Models" tab, one row per agent type) filters its
provider dropdown to `connected-providers` and its model/thinking-level dropdowns
to what omp's `get_available_models` reports for the selected provider (plus omp's
`off|minimal|…|xhigh` thinking-level scale). Prompt overrides get their
own settings surface backed by `GET/PUT/DELETE /api/settings/prompts[/:name]`
(list with `isCustomized`, fetch content + default + variable allowlist + mtime,
save with optimistic-concurrency `expectedMtime` 409 and unknown-variable 400,
delete to revert) — see
[`../reference/server/routes/settings.ts`](../reference/server/routes/settings.ts).

---

## What to build

- [ ] A prompt template registry (name, label, kind, file, allowlisted
      variables) as the single source of truth for which prompts exist.
- [ ] A two-tier loader: bundled default vs override-directory file; override
      wins on presence. Plus `save`/`delete`/`hasOverride`.
- [ ] A `{{var}}` template engine that **throws on missing variables**, plus
      variable extraction and an against-the-allowlist validator (skipped for
      verbatim templates).
- [ ] Per-agent message composition that pre-builds dynamic sections in code and
      injects them via render — and a way to reference one active prompt's path
      from another (the plan-template path).
- [ ] A per-user `Record<AgentType, {provider, model, thinkingLevel}>` store (one
      JSON row per user) with strict load-time validation that **fails loud**, no
      silent default.
- [ ] Model namespace sourced live from omp's `get_available_models` (never a
      hardcoded list), with thinking levels from omp's `off…xhigh` scale.
- [ ] Run-start resolution: load the setting, validate the user's omp credential
      for the provider (typed missing-credentials error), stamp
      `(provider, model, thinkingLevel)` on the run + conversation rows, apply them
      to the omp subprocess via `set_model` / `set_thinking_level`.
- [ ] Deterministic resume: read model off the stored row; only re-resolve from
      the resuming user when the provider matches; never infer.
- [ ] Seed-on-connect + a one-shot backfill, gated by a blocking first-login
      provider modal so no unseeded user can trigger a run.
- [ ] Settings UI: an Agent Models tab (provider/model/thinking-level per agent,
      dropdowns scoped to connected providers + omp's live model list) and a
      prompt-editor surface.

## Reference map

| Concern | File |
|---|---|
| Template engine + override lookup | `reference/server/services/promptRenderer.ts` |
| Per-agent message composition | `reference/server/constants/agentPrompts.ts` |
| Default prompts / templates | `reference/server/constants/{prompts,templates}/*.md` |
| Per-user settings type + seeding helpers | `reference/shared/types/agentModelSettings.ts` |
| Settings load/save/seed/resume resolution | `reference/server/services/agentModelSettings.ts` |
| Run-start resolution + stamping | `reference/server/services/agentRunner.ts` (`loadAgentModelSettings`, `conversationsDb.create`) |
| Settings table | `reference/server/database/init.sql` (`user_agent_model_settings`) |
| Prompt-override HTTP routes | `reference/server/routes/settings.ts` |
| Per-user model HTTP routes | `reference/server/routes/userAgentModelSettings.ts` |
| omp model list / model + thinking-level selection | [`../core/omp-integration.md`](../core/omp-integration.md) (`get_available_models`, `set_model`, `set_thinking_level`) |

## Boundaries (not in this spec)

- How `(provider, model, thinkingLevel)` is applied to the omp subprocess, the
  per-user agent dir, env-injected credentials, and the uniform feature set →
  [`../core/omp-integration.md`](../core/omp-integration.md).
- Where per-user credentials are collected (the provider-connect / omp `/login`
  flow), and the tech/non-tech role logic behind the planning-prompt split →
  [`./auth-and-multi-user.md`](./auth-and-multi-user.md).
- The `startAgentRun` entry point itself, chaining, and the run/conversation row
  lifecycle → [`../core/orchestration-loop.md`](../core/orchestration-loop.md).
- The prompt *content* and the agents' behavioral contracts →
  [`../core/planning-agent.md`](../core/planning-agent.md),
  [`../core/execution-loop.md`](../core/execution-loop.md).
