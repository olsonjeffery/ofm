# Extra — Prompt and model customization

Two independent customization layers sit on top of the provider abstraction.
Neither is required; core ships fixed prompts and `omp` is the default harness
(see [`../core/omp-integration.md`](../core/omp-integration.md)). This extra
makes both **configurable** — the *what an agent says* and the *what runs it*.

> **Implementation status:** The harness-model config layer is **partially
> implemented** via the `agent_harness_configs` table, scope-precedence config
> resolution (`src/providers/registry.rs`), and `LlmProvider` trait abstraction
> (`src/providers/mod.rs`). The agent-level `model` and `effort` fields can be
> set per scope (global/user/project/user-project). The prompt-override layer
> (two-tier file loader, template engine, per-agent message composition) is
> **not yet implemented**.

## What it adds

1. **Prompt overrides.** Every agent prompt is a markdown template with
   `{{variable}}` placeholders. Defaults ship with the app; a user (or operator)
   can drop a same-named file in an override directory to replace any one of them
   without touching code. A small template engine renders them per run.
2. **Per-user model/effort selection.** Each user stores a full
   `Record<AgentType, {provider, model, effort}>` — which LLM backend, which
   model, and how much reasoning effort each of the six agent roles runs on.
   Core resolves this at run start. The cardinal rule: **the model is always
   explicit and resolved deterministically**, never defaulted or inferred at the
   SDK boundary.

The two layers are orthogonal — you can take prompt overrides without per-user
models, or vice versa — but they share one seam: the agent's turn input. The
prompt layer decides the *message* the agent receives; the model layer decides
the `(provider, model, effort)` triple the harness receives. Both are assembled
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
the on-disk `file`, and the **allowlisted variable set** that the template may
reference. See the `PROMPT_DEFINITIONS` array in
[`../reference/server/services/promptRenderer.ts`](../reference/server/services/promptRenderer.ts).
The registry is what the settings UI lists and what variable-validation checks
against.

### The override lookup: default vs `~/.ofm/prompts/`

Resolution is two-tier and dead simple. For a prompt named `X`:

- The **default** lives at `server/constants/{prompts,templates}/X.md` (bundled
  with the app via `include_str!`).
- An **override** may live at `<archiveRoot>/{prompts,templates}/X.md`, where
  `archiveRoot` is `$BOTTEGA_ARCHIVE_ROOT` or `~/.ofm` by default.

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

## Layer 2 — Per-user model and effort

### One settings blob per user, six agents each

Each user owns one row in `user_agent_model_settings` (`user_id` PK,
`settings_json` TEXT) holding their full
`Record<AgentType, {provider, model, effort}>` for the six agent types with
settings (`planification`, `implementation`, `refinement`, `review`, `pr`,
`yolo`). See the table in
[`../reference/server/database/init.sql`](../reference/server/database/init.sql)
and the type in
[`../reference/shared/types/agentModelSettings.ts`](../reference/shared/types/agentModelSettings.ts).
This **replaced** a single global `app_settings.agent_model_settings` blob — the
point of going per-user is that each user runs agents on a provider/model they
actually hold credentials for.

### The model and effort namespaces

`omp` models the available models through its `models.yml` configuration. Each
model entry specifies its provider, model id, and supported features. The
settings UI reads available models from `omp`'s model catalog.

Capabilities are compile-time constants (see [`../extra/harnesses/omp.md`](../extra/harnesses/omp.md)).
This spec only picks the `(model, effort)`; capability guards are core.

### Resolution at run start — `loadAgentModelSettings(userId)`

[`../reference/server/services/agentModelSettings.ts`](../reference/server/services/agentModelSettings.ts)
owns resolution. `loadAgentModelSettings(userId)` reads the row, parses the JSON,
and validates **every one of the six entries**; a missing row, unparseable JSON,
or any invalid entry throws `MissingUserAgentSettingsError` — it **never returns
a silent default**. `startAgentRun` then:

1. Resolves the acting user (fails if there is none — there's no user to resolve
   settings for).
2. Pulls `loadAgentModelSettings(userId)[agentType]` → `(model, effort)`.
3. **Validates credentials up front** via `omp`'s configuration mechanism
   (`models.yml`), surfacing a typed error if the model's auth is not configured.
4. **Stamps `(model, effort)` onto the new `task_agent_runs` and
   `conversations` rows** before starting the turn, then passes them into
   `omp` as part of the turn input.

See `agentRunner.ts` around the `loadAgentModelSettings` call and the
`conversationsDb.create(taskId, provider, model, effort)` line. Stamping the
conversation row is what makes the model **deterministic on resume**.

### The deterministic-model rule

This is the load-bearing invariant, and it mirrors the core `omp-integration.md`
contract: a turn **never runs on a defaulted or inferred model**. On start, the
model comes from the user's setting; on resume, it comes from the stored
conversation row, never re-derived. Any mismatch, an unseeded resuming user, a
manual chat, or a programmatic resume falls back to the row's stored values.
The rule is "fail loud, don't guess."

### Seeding and backfill

Because resolution fails loud, every user must be seeded **before** they can run
an agent. Two mechanisms:

- **Seed on first configuration.** A user's first visit to the settings page
  seeds default model selections for each agent type, sourced from `omp`'s
  available models.
- **Backfill.** Existing users without a settings row are seeded with sensible
  defaults on first access.

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
provider dropdown to `connected-providers` and its model/effort dropdowns to the
selected provider's `MODELS_FOR_UI`/`EFFORTS_FOR_UI`. Prompt overrides get their
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
- [ ] A per-user `Record<AgentType, {model, effort}>` store (one JSON
      row per user) with strict load-time validation that **fails loud**, no
      silent default.
- [ ] Run-start resolution: load the setting, validate model availability
      through `omp`'s configuration, stamp `(model, effort)` on the
      run + conversation rows, pass them into the turn input.
- [ ] Deterministic resume: read model off the stored row; never infer.
- [ ] Seed-on-first-configuration: create default settings for a new user on
      first access so no unseeded user can trigger a run.
- [ ] Settings UI: a Model/Effort picker per agent type, dropdowns scoped to
      models available via `omp`, and a prompt-editor surface.

## Reference map

| Concern | File |
|---|---|
| Template engine + override lookup | `reference/server/services/promptRenderer.ts` |
| Per-agent message composition | `reference/server/constants/agentPrompts.ts` |
| Default prompts / templates | `reference/server/constants/{prompts,templates}/*.md` |
| Per-user settings type + seeding helpers | `reference/shared/types/agentModelSettings.ts` |
| Settings load/save/resume resolution | `reference/server/services/agentModelSettings.ts` |
| Run-start resolution + stamping | `reference/server/services/agentRunner.ts` (`loadAgentModelSettings`, `conversationsDb.create`) |
| Settings table | `reference/server/database/init.sql` (`user_agent_model_settings`) |
| Prompt-override HTTP routes | `reference/server/routes/settings.ts` |
| Per-user model HTTP routes | `reference/server/routes/userAgentModelSettings.ts` |

## Boundaries (not in this spec)

- The direct `omp` integration, the per-turn input shape `(model, effort, prompt)`
  feeds, and the capability constants that gate `omp`-specific features →
  [`../core/omp-integration.md`](../core/omp-integration.md).
- Where per-user credentials come from and how `env` is built for the SDK, and
  the tech/non-tech role logic behind the planning-prompt split →
  [`./auth-and-multi-user.md`](./auth-and-multi-user.md).
- Concrete `omp` integration patterns (subprocess lifecycle, event mapping,
  transcript mirroring, credential delegation) → [`./harnesses/omp.md`](./harnesses/omp.md).
- The `startAgentRun` entry point itself, chaining, and the run/conversation row
  lifecycle → [`../core/orchestration-loop.md`](../core/orchestration-loop.md).
- The prompt *content* and the agents' behavioral contracts →
  [`../core/planning-agent.md`](../core/planning-agent.md),
  [`../core/execution-loop.md`](../core/execution-loop.md).
