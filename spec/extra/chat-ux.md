# Chat UX — manual-chat conveniences

A grab-bag of quality-of-life features for the **manual chat** experience:
slash commands, file attachments, image attachments, voice input, automatic
conversation titling, and the live context-usage meter. None of them is needed
to plan, implement, review, or open a PR.

## What it adds

Five independent conveniences, layered on top of the manual-chat conversation
runtime a human drives by hand:

> **Task 204 addition: Mid-turn question support.** The opencode provider now
> handles `question.asked` SSE events. When opencode asks a question mid-turn,
> the SSE connection is paused (the reader returns without sending `Done`), the
> `QuestionAsked` event is rendered in the message stream as a styled box with
> option tags, and the permanently-visible chat status bar switches to its idle
> "Agent Idle" state (with the Stop Agent button disabled). The user replies via
> the chat input; `resume_turn` POSTs the reply to `/session/{id}/message` and
> opens a fresh SSE connection to continue reading events. The
> `provider_session_id` column (provider-agnostic rename of `omp_session_id`)
> persists the real session ID emitted by `SessionStart`, enabling session
> continuity across restarts via lazy provider recreation.

- **Slash commands** — type `/foo args`, get the body of a markdown command file
  expanded inline before the turn runs.
- **File attachments** — drop a file into a conversation (it lands in the
  worktree's `tmp/` dir, or a task's `input_files/`); the agent reads it from
  disk.
- **Image attachments** — paste/upload images onto a message (Claude-only).
- **Voice input** — hold to record, transcribe speech to text, drop it in the
  input box.
- **Title generation** — auto-name a conversation from its first message.
  Implemented via `generate_conversation_title` in `src/providers/mod.rs`.
- **Context-usage meter** — a live token/context breakdown shown in the chat UI
  (Claude-only for the detailed breakdown).

## Why it's an extra (not core)

The [orchestration loop](../core/orchestration-loop.md) never sees any of this:
agents run from prompts the orchestrator builds, not from a human typing into a
box. These are conveniences for the manual-chat conversation runtime described
in [`opencode.md`](./harnesses/opencode.md). Skip them and core still
plans, implements, reviews, and ships. Several are also **capability-gated** — the
capability constants in [`opencode.md`](./harnesses/opencode.md) decide which ones light up.

## Slash commands

Let a user type `/review-checklist some-arg` and have the conversation see the
**body of `review-checklist.md`** instead, with arguments substituted.

- **Discovery** is filesystem-based. Command files are markdown under
  `.claude/commands/` — first the project's repo, then `~/.claude/commands/`
  (user-global). Subdirectories namespace the command name (`db/migrate.md` →
  `/db/migrate`). The list endpoint
  ([`reference/server/routes/commands.ts`](../reference/server/routes/commands.ts),
  `POST /commands/list` → `scanCommandsDirectory`) walks both trees, parses YAML
  frontmatter with `gray-matter`, and derives each command's `description` from
  the frontmatter or the file's first heading.
- **Expansion** happens server-side, just before the turn is sent.
  `resolveSlashCommand(message, projectPath)`
  ([`reference/server/services/conversation/slashCommands.ts`](../reference/server/services/conversation/slashCommands.ts))
  short-circuits unless the message starts with `/`, looks up
  `<name>.md` (or `<name>/index.md`) in the same project-then-user search dirs,
  strips the frontmatter, and substitutes `$ARGUMENTS` (all args joined) plus
  positional `$1`, `$2`, … On a miss it returns the message unchanged — a
  literal `/foo` just gets sent verbatim. It is called inside the conversation
  starters (`resolveSlashCommand(finalMessage, projectPath)` in
  [`startConversation.ts`](../reference/server/services/conversation/startConversation.ts)),
  after image handling and before the prompt is delivered.
- **Frontend** is a typeahead menu:
  [`reference/src/hooks/useSlashCommands.ts`](../reference/src/hooks/useSlashCommands.ts)
  fetches the list per project, filters on the text after `/`, and inserts the
  chosen command name back into the input. The actual expansion is still the
  server's job — the hook only helps the user type the name.

The two halves use the **same search-dir convention** (project `.claude/commands`
then `~/.claude/commands`); keep them in sync or the menu will list commands the
resolver can't find.

## File attachments

Two destinations, same idea: get a file onto disk where the agent's shell can
read it, and tell the agent where it is.

- **Per-conversation upload** → the worktree's `tmp/` dir.
  `saveConversationUpload(repoPath, filename, buffer)`
  ([`reference/server/services/documentation.ts`](../reference/server/services/documentation.ts))
  sanitizes the filename, writes it under `<repo>/tmp/`, and returns a
  `relativePath` like `./tmp/foo.txt`. The HTTP entry point is
  `POST /projects/:id/upload`
  ([`reference/server/routes/projects.ts`](../reference/server/routes/projects.ts),
  ~L169-205), using the in-memory `multer` middleware
  ([`reference/server/middleware/upload.ts`](../reference/server/middleware/upload.ts)).
- **Per-task input files** → a task's central archive.
  `saveTaskInputFile` / `listTaskInputFiles` / `deleteTaskInputFile` (same
  `documentation.ts`) write into
  `~/.bottega/projects/<id>/tasks/task-<id>/input_files/`, which lives *outside*
  the repo so it survives worktree destruction on merge. `buildContextPrompt`
  injects an "Input Files" section listing those files with an instruction to
  read them before doing anything — that is how the agent is told to pick them
  up.

Both paths just persist bytes and surface a path; the agent reads them with its
ordinary file tools.

## Image attachments — Claude-only

Images ride along on a message rather than being uploaded separately. The
WebSocket message carries `images: Array<{ data: string; mimeType: string }>`
(`data` is a base64 data-URI; see
[`reference/server/services/conversation/types.ts`](../reference/server/services/conversation/types.ts),
`ConversationImage`). `handleImages(command, images, cwd)`
([`reference/server/services/conversation/media.ts`](../reference/server/services/conversation/media.ts))
decodes each data-URI to a temp file under `<cwd>/.tmp/images/<ts>/` and appends
an `[Images provided at the following paths:]` note to the message so the agent
reads them off disk; `cleanupTempFiles` removes them after the turn.

This is gated on **`supportsImages`** from the capability matrix
([`reference/shared/providers/capabilities.ts`](../reference/shared/providers/capabilities.ts)).
Anthropic sets it `true`; Codex and OpenCode set it `false` and **silently strip
attached images** (see the comments around the `handleImages` call in
[`startCodexConversation.ts`](../reference/server/services/conversation/startCodexConversation.ts)
and
[`startOpenCodeConversation.ts`](../reference/server/services/conversation/startOpenCodeConversation.ts)).
The chat UI should disable the image affordance when the active provider can't do
images, so a user never silently loses an attachment.

## Voice input — needs `OPENAI_API_KEY`

Record audio in the browser, transcribe it server-side, drop the text in the
input box (the user still presses send). It is **independent of the coding
harness** — it always uses OpenAI, regardless of which provider runs the turn.

- **Backend:** `POST /transcribe` (multer memory upload of an `audio` field, in
  [`reference/server/index.ts`](../reference/server/index.ts) ~L306) calls
  `transcribeAudio(buffer)`
  ([`reference/server/services/transcription.ts`](../reference/server/services/transcription.ts)):
  remux the `.webm` to mp3 with ffmpeg, transcribe with OpenAI
  **`gpt-4o-transcribe`**, then run a second `gpt-4o-mini` pass that *cleans up*
  the transcript (fixes filler words, never answers the question — the system
  prompt is emphatic about transcribe-don't-respond). Requires `OPENAI_API_KEY`;
  throws a clear error if it's unset.
- **Frontend:** `MicButton` records via `MediaRecorder`, posts the blob through
  `transcribeWithWhisper(blob)`
  ([`reference/src/utils/whisper.ts`](../reference/src/utils/whisper.ts)), and
  `MessageInput` inserts the returned text.

## Title generation

Auto-name an untitled conversation so the sidebar isn't a wall of "New chat."
**Fire-and-forget** — it must never block or fail the turn.

`generateConversationTitle(...)`
([`reference/server/services/titleGenerator.ts`](../reference/server/services/titleGenerator.ts))
spawns the `claude` CLI with the **Haiku** model and `--max-turns 1` on the
conversation's first user message, sanitises the output (strip quotes/trailing
punctuation, cap ~50 chars), writes it to the conversation row, and dual-emits
`conversation-name-updated` on both the conversation channel (chat header) and
the task channel (the task viewer's conversation list). It is invoked from the
`onSessionId` callback in
[`startConversation.ts`](../reference/server/services/conversation/startConversation.ts)
(~L233). If credentials are missing or the CLI errors/times out (20s) it just
logs and returns — the conversation is unaffected.

> The reference titler shells out to the `claude` CLI directly rather than going
> through the streaming runtime. For `ofm`, route title generation through
> the opencode provider session instead. It is a cosmetic nicety either way.
>
> **ofm call sites:** `generate_conversation_title()` is called from
> `orchestration/mod.rs` (on `SessionStart` in the auto-start path) and
> `server/routes/conversations.rs` (on first user message in a resumed
> conversation, gated on `conv.name.is_none()`). Both are fire-and-forget
> `tokio::spawn` calls. A `conversation-name-updated` WS event is broadcast
> on the task topic after the title is persisted.

## Context-usage meter

A live readout of how full the model's context window is, with an optional
per-category breakdown (system prompt, MCP tools, memory files, …) in a modal.

`createContextUsageTracker({ conversationId, broadcastFn })`
([`reference/server/services/contextUsageTracker.ts`](../reference/server/services/contextUsageTracker.ts))
is created per streaming session and fed by the shared event consumer. It uses a
**hybrid baseline+breakdown** design, and the *why* is load-bearing:

- The **baseline** (total/max tokens, percentage, model) is computed from the
  terminal `result` event's `modelUsage` — this **always works**.
- The **breakdown** (categories, MCP tools, system-prompt sections) comes from
  the SDK's `getContextUsage()` control call, captured mid-stream on a master
  assistant event. Because Bottega spawns a one-shot subprocess per turn, that
  call frequently loses the race against subprocess teardown, so the breakdown is
  only *folded in when it wins*. Sub-agent assistant events (non-null
  `parent_tool_use_id`) are skipped so they can't clobber the master's totals.

The result is persisted to `conversations.context_usage_json` and broadcast as a
`context-usage` WebSocket message; the frontend
([`reference/src/components/ChatInterface.tsx`](../reference/src/components/ChatInterface.tsx)
`handleContextUsage`, and
[`reference/src/components/ContextDetailModal.tsx`](../reference/src/components/ContextDetailModal.tsx))
renders it and refetches the last snapshot on load.

The detailed breakdown is gated on **`supportsContextUsageBreakdown`** (Claude
only). On Codex/OpenCode the tracker still emits a baseline from aggregate usage,
but there is no per-category detail — the modal should degrade to the bar, not
break.

## What to build

- [ ] Slash-command discovery (`POST /commands/list`) and server-side expansion
      (`resolveSlashCommand`) over a shared project-then-user `.claude/commands`
      search path, with `$ARGUMENTS`/`$N` substitution; a typeahead hook on the
      frontend.
- [ ] File upload to the worktree `tmp/` dir and to a task's central
      `input_files/`, with the latter announced in the task context prompt.
- [ ] Image attachments decoded to temp files and referenced by path — gated on
      `supportsImages`, silently dropped (and UI-disabled) otherwise.
- [ ] A `/transcribe` endpoint backed by `gpt-4o-transcribe` (+ a cleanup pass),
      a browser recorder, requiring `OPENAI_API_KEY`.
- [x] Fire-and-forget title generation on first message, writing to the
      conversation row; never blocking the turn.
      → Called from `src/orchestration/mod.rs` (SessionStart) and
      `src/server/routes/conversations.rs` (first user message on resume)
- [ ] A per-session context-usage tracker with a baseline-from-`result` path and
      an optional breakdown folded in when the control call wins; persist +
      broadcast; gate the detailed breakdown on `supportsContextUsageBreakdown`.

## Reference map

| Concern | File |
|---|---|
| Slash-command expansion | `reference/server/services/conversation/slashCommands.ts` |
| Slash-command listing route | `reference/server/routes/commands.ts` |
| Slash-command typeahead hook | `reference/src/hooks/useSlashCommands.ts` |
| File/image temp handling | `reference/server/services/conversation/media.ts` |
| Uploads + task input files | `reference/server/services/documentation.ts`, `reference/server/routes/projects.ts`, `reference/server/middleware/upload.ts` |
| Voice transcription (backend) | `reference/server/services/transcription.ts`, `reference/server/index.ts` (`/transcribe`) |
| Voice transcription (frontend) | `reference/src/utils/whisper.ts`, `reference/src/components/MicButton.tsx` |
| Title generation | `reference/server/services/titleGenerator.ts` |
| Context-usage tracker | `reference/server/services/contextUsageTracker.ts` |
| Context-usage UI | `reference/src/components/ChatInterface.tsx`, `reference/src/components/ContextDetailModal.tsx` |
| Capability matrix (gates images + breakdown) | `reference/shared/providers/capabilities.ts` |

## Message stream styling

The conversation message stream applies distinct visual styling per content type, enforced via CSS classes in `src/webapp/styles/app.css` (both SSR and JS rendering paths produce identical HTML):

| Content Type | CSS Class | Icon | Theme |
|---|---|---|---|---|
| Model statement | `.message-model` | None | Default text, semi-bold (600) |
| Thinking | `.message-thinking` | `mdi-snowflake-outline` | Purple background/border/text, flexbox icon+content layout |
| Tool usage (unified card) | `.message-tool` | `mdi-cog-outline` | Gray background/border/text. Shows Input and Result sections with `tool-section-label` labels, separated by `<hr>`. Each section independently collapsible. |
| User input | `.message-user` | None | Blue (#1565c0) background, white text, right-aligned, 45% max-width |
| Question asked | `.message-question` | `mdi-help-circle-outline` | `is-info is-light` notification (blue info palette) |

**Show More/Less** — any box with content exceeding 256 characters renders in a collapsed state:
- A truncated preview (first 256 chars) is shown by default, with a `…` ellipsis appended.
- A right-aligned `.show-more-btn` link toggles display of the full content via `toggleShowMore(id)` or `toggleShowMoreLines(id, count)`.
- Clicking the button switches between "show more" and "show less" text.

**Chunk suppression** — `TextChunk` and `ThinkingChunk` events are silently dropped in the SSR (`message_stream.rs` `render_event`) rendering path. Only final `Text` / `Thinking` events appear in the stream.

**Deduplication** — both server-side and client-side dedup prevent duplicate content:

- **Server-side (SSR):** `MessageStream` in `message_stream.rs` applies a `HashSet` fingerprint before calling `render_event()`. Fingerprint keys mirror the JS pattern: `"text:{text}"`, `"user_text:{text}"`, `"thinking:{thinking}"`. Tool events use **prefixed keys** (`"use:{tool_use_id}"` for ToolUse, `"result:{tool_use_id}"` for ToolResult) so that both variants survive dedup independently (fixed in Task 67). After dedup, a **merge pass** iterates the dedup'd events and combines adjacent ToolUse+ToolResult pairs by matching `tool_use_id` into a single unified ToolUse card with both input and result. This handles old-format DB rows (separate ToolUse+ToolResult).
- **Client-side (WS fallback):** JS in `chat.rs` maintains a `renderedMessageIds` map for `tool_use_id` dedup. For `tool_updated` events, the entire card's `outerHTML` is replaced with server-rendered HTML. For `tool_use` and `tool_result` events without pre-rendered HTML, the old `updateToolCallContent` merge logic is kept as a fallback. Duplicate `user_text` broadcasts are dropped by a `lastUserText` text-match guard in the JS WS handler, immediately after the `conversation_id` filter (Task 81).

### `tool_updated` Event Type

ToolUse events with a populated `result` field emit a `tool_updated` WS event type (instead of `tool_use`). The client JS detects `tool_updated` in the WS handler and replaces the tool card's `outerHTML` with the server-rendered unified HTML from `msg.html`. This ensures the client view matches what a page reload would show.

### Unified Tool Card Merging

Tool cards (`.message-tool`) now display tool name, input, and result in a single layout:

```
[icon] tool_name
  Input:
    {prettified input JSON}
  ---
  Result:
    {result content}
```

- If input is null and result is `None`: the card is suppressed entirely (Phase 2a).
- If `result` is `Some`: both Input and Result sections render with independent collapse/show-more logic, separated by an `<hr>`.
- If `result` is `None`: only the Input section renders.

The `.tool-section-label` CSS class styles the "Input:" / "Result:" labels at 0.85em with muted color.

## Navbar agent status dropdown (global live agent feed)

The navbar **AgentDropdown** (`src/webapp/components/agent_dropdown.rs`) is the
single global view of agent activity, present on **every** page. It is driven
exclusively by server activity, never by local client-side events:

- The component subscribes to the WebSocket **System** topic
  (`WsTopic { kind: System, id: 0 }`), which every agent state transition
  publishes an `agent_status` event onto:
  - **Agent start** — `start_next_agent` broadcasts `refresh`
  - **Agent completed** — `completion_handler` broadcasts `completed`
  - **Agent failed** (unexpected session end) — the broadcast task cleanup in
    `src/orchestration/mod.rs` and `src/server/routes/conversations.rs`
    broadcasts `failed`
  - **Agent start failure** — `start_next_agent`'s `start_turn` error path
  - **Stop Agent** — `reset_agent_runs` broadcasts `stopped`
  - **Run re-activated by a chat message** — `send_message` broadcasts
    `refresh`
  - **Question asked** (agent paused waiting for input) — the broadcast event
    loop broadcasts `question`
  - **Run blocked** (missing provider config) — `start_next_agent` broadcasts
    `blocked`
- On any `agent_status` event (and on page load / WS reconnect), the dropdown
  re-fetches `GET /api/tasks/agent-status` and re-renders:
  - **Button**: running-agent count; a 15-second pulse on the message-outline
    icon while ≥ 1 agent runs.
  - **Menu**: the live WebSocket connection status, one entry per running agent
    (linking to its chat), a "Needs your input" section listing open-question
    tasks, and a "Blocked" section listing blocked tasks.
  - **Styling**: the whole trigger is tinted `--bulma-cyan` when ≥ 1 open
    question task, and `is-primary` when ≥ 1 blocked task — blocked **trumps**
    every other rule.
- A 30-second re-sync poll guards against a dropped frame; server broadcasts
  remain the source of truth.

**Open-question detection** (`get_open_question_tasks` in
`src/services/tasks.rs`): a task has an open question when a conversation's
newest persisted message is a `question_asked` event — i.e. the agent paused and
the user has not yet replied.

**Blocked detection** (`get_blocked_tasks`): tasks with `workflow_blocked = 1`
(the server-only iteration-cap marker) or with a run stuck in `blocked` status.

## Boundaries (not in this spec)

- The conversation runtime that these hook into — streaming, transcript
  persistence, and the RPC protocol → [`../extra/harnesses/opencode.md`](../extra/harnesses/opencode.md).
- The autonomous agent pipeline (none of these features touch it) →
  [`../core/orchestration-loop.md`](../core/orchestration-loop.md).
- The task doc and `input_files/` lifecycle and where the archive lives →
  [`../core/task-and-workspace.md`](../core/task-and-workspace.md).
- Harness capability values and how they are advertised →
  [`./harnesses/opencode.md`](./harnesses/opencode.md).
