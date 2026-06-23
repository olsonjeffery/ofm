# Chat UX — manual-chat conveniences

A grab-bag of quality-of-life features for the **manual chat** experience:
slash commands, file attachments, image attachments, voice input, automatic
conversation titling, and the live context-usage meter. None of them is needed
to plan, implement, review, or open a PR.

## What it adds

Five independent conveniences, layered on top of the manual-chat conversation
runtime a human drives by hand:

- **Slash commands** — type `/foo args`, get the body of a markdown command file
  expanded inline before the turn runs.
- **File attachments** — drop a file into a conversation (it lands in the
  worktree's `tmp/` dir, or a task's `input_files/`); the agent reads it from
  disk.
- **Image attachments** — paste/upload images onto a message; they ride along on
  the `prompt`/`steer`/`follow_up` frame as `images`.
- **Voice input** — hold to record, transcribe speech to text, drop it in the
  input box.
- **Title generation** — auto-name a conversation from its first message.
- **Context-usage meter** — a live token/context breakdown shown in the chat UI,
  fed by omp's `get_state.contextUsage`.

## Why it's an extra (not core)

The [orchestration loop](../core/orchestration-loop.md) never sees any of this:
agents run from prompts the orchestrator builds, not from a human typing into a
box. These are conveniences for the manual-chat conversation runtime described
in the [omp integration](../core/omp-integration.md). Skip them and core still
plans, implements, reviews, and ships. They lean on omp's RPC surface directly —
image attachments on the `prompt` frame, the context meter on
`get_state.contextUsage` — without any capability gating, because omp is the only
backend.

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
  sanitises the filename, writes it under `<repo>/tmp/`, and returns a
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

## Image attachments

Images ride along on a message rather than being uploaded separately. The
WebSocket message carries `images: Array<{ data: string; mimeType: string }>`
(`data` is a base64 data-URI; see
[`reference/server/services/conversation/types.ts`](../reference/server/services/conversation/types.ts),
`ConversationImage`).

omp's RPC `prompt` / `steer` / `follow_up` commands accept an
`images?: ImageContent[]` field directly, so the cleanest path is to pass the
decoded images on the command frame. (Fallback, when on-frame images aren't
wired: the reference's `handleImages` decodes each data-URI to a temp file under
`<cwd>/.tmp/images/<ts>/` and appends an `[Images provided at the following
paths:]` note for the agent to read off disk, cleaned up by `cleanupTempFiles`
after the turn.)

Vision support depends on the **selected model**, not on omp, and there is no
provider-level capability flag to consult — so the chat UI disables the affordance
per model where the chosen model lacks vision.

## Voice input — needs `OPENAI_API_KEY`

Record audio in the browser, transcribe it server-side, drop the text in the
input box (the user still presses send). It is **independent of the coding
agent** — it always uses OpenAI for transcription, regardless of which model omp
runs the turn on.

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

> The reference titler shells out to the `claude` CLI directly. In `omprint`,
> generate the title through `omp` instead — either a short one-shot `omp -p`
> invocation on the first user message, or a cheap `smol`-role prompt over the
> RPC channel. (omp disables its *own* automatic session titling in RPC mode, so
> this stays omprint's job.) It is a cosmetic nicety either way.

## Context-usage meter

A live readout of how full the model's context window is, with an optional
per-category breakdown (system prompt, MCP tools, memory files, …) in a modal.

`createContextUsageTracker({ conversationId, broadcastFn })`
([`reference/server/services/contextUsageTracker.ts`](../reference/server/services/contextUsageTracker.ts))
is created per streaming session and fed by the shared event consumer:

- omp's `{ type: "get_state" }` response carries
  `contextUsage: { tokens, contextWindow, percent }`, which is the baseline
  readout (current tokens, the model's context window, and the fill percentage).
  Poll/read it as the turn streams to keep the meter live.
- Per-turn usage in omp's terminal events refines the count at turn end.

The result is persisted to `conversations.context_usage_json` and broadcast as a
`context-usage` WebSocket message; the frontend
([`reference/src/components/ChatInterface.tsx`](../reference/src/components/ChatInterface.tsx)
`handleContextUsage`, and
[`reference/src/components/ContextDetailModal.tsx`](../reference/src/components/ContextDetailModal.tsx))
renders it and refetches the last snapshot on load.

There is no provider capability gate: omp always reports `contextUsage` via
`get_state`, so the meter is always available.

## What to build

- [ ] Slash-command discovery (`POST /commands/list`) and server-side expansion
      (`resolveSlashCommand`) over a shared project-then-user `.claude/commands`
      search path, with `$ARGUMENTS`/`$N` substitution; a typeahead hook on the
      frontend.
- [ ] File upload to the worktree `tmp/` dir and to a task's central
      `input_files/`, with the latter announced in the task context prompt.
- [ ] Image attachments passed on the omp `prompt`/`steer`/`follow_up` frame's
      `images` field (or decoded to temp files and referenced by path as a
      fallback); UI-disabled per-model where the chosen model lacks vision.
- [ ] A `/transcribe` endpoint backed by `gpt-4o-transcribe` (+ a cleanup pass),
      a browser recorder, requiring `OPENAI_API_KEY`.
- [ ] Fire-and-forget title generation on first message, broadcast on both the
      conversation and task channels; never blocking the turn.
- [ ] A per-session context-usage tracker fed by omp's `get_state.contextUsage`
      (`{ tokens, contextWindow, percent }`), refined by per-turn usage at turn
      end; persist + broadcast.

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

## Boundaries (not in this spec)

- The conversation runtime that these hook into — how omp is spawned and driven,
  streaming, the event format, and transcript persistence →
  [`../core/omp-integration.md`](../core/omp-integration.md).
- The autonomous agent pipeline (none of these features touch it) →
  [`../core/orchestration-loop.md`](../core/orchestration-loop.md).
- The task doc and `input_files/` lifecycle and where the archive lives →
  [`../core/task-and-workspace.md`](../core/task-and-workspace.md).
