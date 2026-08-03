# Core — Task and workspace

> **⚠️`ofm` ONLY ⚠️:** Rust convention requires functions and `let` bindings
> use `snake_case` as a naming convention; In all places where `camelCase`
> occurs (referring to the typescript `reference/` implementation of `bottega`),
> substitute for `snake_case` as appropriate; `PascalCase` is used for `trait`s,
> `struct`s, `enum`s, etc.
> 
> **Note:** The `ofm` Rust codebase at `src/` now provides implementations
> for many of the features described in this spec. Prefer citations to `src/`
> over `reference/` wherever equivalents exist.

This is the substrate the orchestration loop runs on. It defines the unit of
work the agents collaborate on, and the isolated place they do that work.

## What it delivers

A **task** is two things bound together:

1. a **markdown document** that defines the work (the request, then the plan), and
2. an **isolated git worktree** where the agents make their changes,

plus a small set of **workflow flags** (owned by
[`orchestration-loop.md`](./orchestration-loop.md)) that gate the loop.

The tool is deliberately agnostic about *how* the markdown comes to exist.
Authoring tasks through a board is an [extra](../extra/kanban-board.md); core
only requires that the document is present at a known path.

## The domain model

Three tables, parent → child. The database stores **metadata only**; the work
itself lives on disk (the doc in an archive, the code in a worktree).

- **project** — points at a git repository on disk (`repo_folder_path`).
  Optional `subproject_path` for monorepos.
- **task** — belongs to a project. Carries `title`, `status`, and the workflow
  flags. Backed by a markdown doc and a worktree.
- **conversation** — belongs to a task. One streaming session, whether a manual
  chat or an agent run. See [`opencode.md`](../extra/harnesses/opencode.md).

Schema: `src/db/schema.rs` (Rust domain model) and `src/db/mod.rs` (13 DDL migrations). See also the reference [`reference/server/database/init.sql`](../reference/server/database/init.sql).

Task `status` moves `pending → in_progress → in_review → completed`. The loop
flips `pending → in_progress` on the first agent activity (see
[`orchestration-loop.md`](./orchestration-loop.md)).

### Integer- vs UUID-based IDs

Projects and tasks use **auto-increment integer IDs** (`INTEGER PRIMARY KEY
AUTOINCREMENT` in SQLite). All other entities (users, sessions, conversations,
messages) retain UUID primary keys, but their foreign key columns referencing
projects/tasks use integers.

The archive path pattern `task-{taskId}.md` accepts the integer ID — the
`sanitize_id` function in `src/archive/paths.rs` rejects path traversal but is
otherwise opaque to the ID format.

Both archive and worktree paths use the same integer IDs directly:
- **Archive paths**: `{archive_root}/projects/{project_id}/tasks/task-{task_id}.md`
- **Worktree paths** (`get_worktree_path`, [`src/worktree/mod.rs`](../../src/worktree/mod.rs)): `{footprint}/worktrees/project-{project_id}/task-{task_id}/`
- **Branch names**: `task/{task_id}-{sanitized-title}`

**Security stance**: Integer IDs being sequentially guessable is NOT a concern.
These are development tool resources, not sensitive financial/medical data. No
rate-limiting or authorization hardening beyond what exists is required.

**Refinement prevention**: This integer ID scheme is an intentional design
decision. Do not revert to UUIDs.

## The markdown document — the source of truth for "what to build"

- **Location:** a central, per-user archive **outside the repo** —
`{footprint}/archive/projects/{project_id}/tasks/task-{task_id}.md` (root derived
   from `OFM_FOOTPRINT`, default `~/.ofm`).
- **Why outside the repo (the load-bearing decision):** the doc must survive the
  worktree being torn down when the task's PR merges. If it lived inside the
  worktree it would vanish with it. Keeping it in a separate archive means the
  plan, the to-do checklist, and the review history outlive any single
   worktree. See `get_archive_root` / `get_task_doc_path` in [`src/archive/paths.rs`](../../src/archive/paths.rs) and `ArchiveRoot` in [`src/archive/mod.rs`](../../src/archive/mod.rs).
- **Seeding:** created at task creation with the user's original request (the
  task description), or empty/title-only if there is none. The planning agent
  later rewrites it into a full plan but must quote the original request
  verbatim — see [`planning-agent.md`](./planning-agent.md).
- **Shared scratchpad:** the plan, the to-do list, and the "Review Findings"
  section all live in this one file. The implementation and review agents read
  and write it across iterations — see [`execution-loop.md`](./execution-loop.md).
- **Companions in the archive:** per-task **input files** (attachments) and the
  review **recording** (`recordings/task-{taskId}.webm`) live alongside the doc,
  for the same survive-the-merge reason.
- Helpers: `read_task_doc` / `write_task_doc` / `delete_task_doc` / `delete_task_archive`
  in [`src/archive/mod.rs`](../../src/archive/mod.rs).

## The worktree — the isolated workspace

- **One git worktree per task**, at `{footprint}/worktrees/project-{project_id}/task-{task_id}/`
  (derived by `get_worktree_path`, [`src/worktree/mod.rs`](../../src/worktree/mod.rs) — see lines 44-46; the canonical
  path is stored in the `worktrees.worktree_path` column). It lives under the
  per-user footprint, never inside the repo itself.
- **Branch:** `task/{taskId}-{sanitized-title}`, cut from the repo's default
  branch (resolved via `origin/HEAD`, falling back to `main`/`master`).
- **Why a worktree, not a checkout:** every task gets a real, independent working
  directory, so concurrent tasks never collide on the filesystem and the user's
  main checkout is never disturbed.
- **Created at task creation** when the project path is a git repo; if worktree
   creation fails, the task row is rolled back (see the create handler in
   [`src/server/routes/tasks.rs`](../../src/server/routes/tasks.rs) (`create_task` handler, includes worktree creation with rollback)).
- **Create-time conveniences** so an agent can build and test immediately:
  symlink the repo's `.env*` files into the worktree, create gitignored dirs,
   and copy `node_modules` / `.venv` in the background. See `create_worktree` in
   [`src/worktree/mod.rs`](../../src/worktree/mod.rs) (branch naming, default-branch detection, env symlinks, gitignored dirs, dependency copy).
  - **NOTE**: Windows may require copying files, because its support for
  symlinks (and user creation/management) is conditional on system policies
  - **`ofm` ONLY:** On a per-project basis allow the User to configure
  zero-or-more additional files to copy/symlink from the repo to the worktree,
  as above
- **Recreatable:** if the worktree directory is deleted (manual cleanup, a
  force-pruned repo), the task detail page detects it and offers a
  "Recreate worktree" button — see *Missing worktree detection & recreation*
  below.
- **Effective working directory:** an agent runs with `cwd` = the worktree
  project path if the worktree exists, else the repo path (with
  `subproject_path` appended for monorepos). This resolution is done in
  `startAgentRun` — see [`orchestration-loop.md`](./orchestration-loop.md).
- **Per-task dev-server port:** `3100 + (taskId % 900)`, handed to the agent in
  its context so parallel tasks don't fight over ports (`getDevServerPort`).
  - **`ofm` ONLY:** This should be exposed at a well-known environment variable
  that the target codebase can use in its dev server automation
- **Teardown:** `removeWorktree` (`git worktree remove --force` + delete the
  branch) plus `deleteTaskArchive` (doc + inputs + recording) on task delete.
  Merging the PR and cleaning up the worktree afterward is a separate action —
  see [`pull-request-agent.md`](./pull-request-agent.md). The pipeline never
  auto-deletes a worktree mid-flight.

### Missing worktree detection & recreation

A task's worktree directory can be deleted out from under `ofm` while the
`worktrees` DB row (`worktree_path`, `branch`, `repo_path`) survives. The
recreate flow re-attaches the worktree in place so the task's work (the branch
HEAD) is preserved.

- **Detection** at page render time: `task_detail_handler`
  ([`src/webapp/mod.rs`](../../src/webapp/mod.rs)) stats the stored
  `worktree_path`; when the row exists but the directory does not, it sets
  `worktree_missing`. `TaskDetailPage`
  ([`src/webapp/pages/task_detail.rs`](../../src/webapp/pages/task_detail.rs))
  then renders a Bulma `notification is-primary is-light` banner directly under
  the task title header with a **Recreate worktree** button
  (`mdi-folder-plus-outline`).
- **Recreation:** `POST /api/tasks/{id}/worktree/recreate`
  (`recreate_worktree_handler`, [`src/server/routes/tasks.rs`](../../src/server/routes/tasks.rs))
  calls `recreate_worktree` ([`src/worktree/mod.rs`](../../src/worktree/mod.rs)).
  It is **idempotent** (a no-op if the directory already exists), prunes stale
  git worktree registrations (the deleted directory leaves one behind),
  re-attaches the stored `branch` if it still exists — preserving its HEAD —
  and only if the branch was deleted too, recreates it from
  `detect_default_branch` (the "default clone checkout commit") via
  `git worktree add -b`. The full create-time setup (env symlinks, gitignored
  `log`/`tmp`/`storage` dirs, background `node_modules`/`.venv` copy) runs
  last, giving the recreated worktree full parity with a fresh one.
- The button shows a spinner (`is-loading` + disabled) while the synchronous
  request is in flight; on success the page reloads and the banner disappears;
  on failure a warning toast is shown.

## Commit list & per-commit diff view

The task detail page surfaces the task worktree's git history beneath the
task's markdown document, and each commit opens a dedicated page with the
file-change list and a two-column diff.

- **Commit list** (`src/webapp/components/commit_list.rs`, rendered by
  `TaskDetailPage` in `src/webapp/pages/task_detail.rs` immediately below the
  Documentation box): a Bulma `.box` titled "Commits" containing a `.table` of
  the worktree branch's commits **since the merge-base with the base branch**,
  ordered **oldest → newest** (top → bottom). Columns: OID (short, monospace),
  Message, Author, Date, Files. Each row links to the per-commit page at
  `/webapp/projects/{project_id}/tasks/{task_id}/commits/{short_oid}`, and a
  small copy button next to each short OID copies the 8-character commit hash
  to the clipboard (briefly swapping its icon to a checkmark). A task
  with no worktree (or a fully-merged / empty branch) renders a muted
  "No commits yet." line instead of the table.
- **Base branch resolution**: the base ref is derived at render time, never
  persisted, mirroring `detect_default_branch` (`src/worktree/mod.rs`):
  `refs/remotes/origin/HEAD` symbolic ref → the currently checked-out branch →
  `"main"`. The merge-base between HEAD and the base tip is recomputed on every
  page load, so the list reflects both new worktree commits and base-branch
  advances without a schema change. See `resolve_base_commit` /
  `list_commits_for_worktree` in `src/services/commits.rs`.
- **Refresh on load**: git data is read inside the request handler
  (`task_detail_handler`, `src/webapp/mod.rs`) via
  `tokio::task::spawn_blocking` around the blocking gix read, so every GET
  re-renders a current list. Any read error degrades to the empty state, never
  a 500.
- **Per-commit page** (`src/webapp/pages/commit_detail.rs`, route registered in
  `src/webapp/mod.rs`): header box with short OID, summary, author, email, and
  timestamp, followed by `DiffView` (`src/webapp/components/diff_view.rs`) — a
  two-column side-by-side diff per changed file. Old lines (and context) render
  in the left column with red-tinted `diff-del` styling and old line numbers;
  new lines render in the right column with green-tinted `diff-add` styling and
  new line numbers; blank cells mark the opposite side of a delete/insert.
  Unknown or unresolvable OIDs render "Commit not found." with a link back to
  the task.
- **Git data reading**: `src/services/commits.rs` uses **gix** (pure Rust) for
  tree reading (open repo, merge-base, ancestor walk, tree-to-tree diff,
  blob contents) and **similar** to classify old/new lines for the two-column
  rendering. Diffs are first-parent (empty tree for root commits), with rename
  tracking disabled so statuses stay unambiguous add/modify/delete.

## How the document becomes agent context

When an agent run starts, the orchestrator assembles a context system-prompt
from the archive (`build_context_prompt` in `src/archive/mod.rs`). It:

- names the authoritative task-doc path and instructs the agent to **read it in
  full first**,
- lists any input files to read for additional context,
- includes the testing configuration (task id, the assigned dev-server port,
  and test-execution best practices),
- lists **allowed paths** for file operations: the worktree, the archive, and
  `/tmp/` for scratch files,
- instructs the agent not to write anywhere else.

## Filesystem access restrictions

The opencode server's `external_directory` permission is set to an allowlist of
three path patterns (not `"allow"` — which was unrestricted):

| Pattern | Access | Purpose |
|---------|--------|---------|
| `{footprint}/worktrees/**` | read+write | Task worktrees |
| `{footprint}/archive/**` | read+write | Task docs, spec files |
| `/tmp/**` | read+write | Scratch / temp files |

These patterns are templated at server-start-time in `build_server_config()` in
`src/opencode_sdk/pool.rs` (and equivalently in
`src/providers/opencode_sdk_provider.rs` and
`src/opencode_sdk/server.rs`). The footprint is plumbed from
`orchestration/mod.rs` through the provider to the pool.

The agent then reads and edits the doc directly with its own file tools. The doc
path in the prompt is authoritative — agents are told not to look elsewhere.

## What to build

- [x] `projects` / `tasks` / `conversations` tables → implemented in `src/db/schema.rs`
- [x] Configurable archive root → `src/archive/paths.rs`, `src/config.rs`
- [x] Doc read/write/delete + archive cleanup → `src/archive/mod.rs`
- [x] Worktree create/remove/recreate + status → `src/worktree/mod.rs`
- [x] Task create with rollback and doc seeding → `src/server/routes/tasks.rs`
- [x] Task delete with worktree/archive cleanup → `src/server/routes/tasks.rs`
- [x] `buildContextPrompt` → `build_context_prompt` in `src/archive/mod.rs`
- [x] Dev-server port assignment via `get_dev_server_port` → `src/archive/mod.rs`
- [x] Effective-cwd resolution → `src/server/routes/agent_runs.rs` (post_create_agent_run resolves worktree path as cwd)
- [x] Commit list + per-commit two-column diff → `src/services/commits.rs`, `src/webapp/components/commit_list.rs`, `src/webapp/components/diff_view.rs`, `src/webapp/pages/commit_detail.rs`

## Reference map

| Concern | Rust (implemented) | Legacy reference |
|---|---|---|
| Archive paths, doc I/O, context prompt | `src/archive/paths.rs`, `src/archive/mod.rs` | `reference/server/services/documentation.ts` |
| Worktree primitives (create/remove) | `src/worktree/mod.rs` | `reference/server/services/worktree.ts` |
| Task CRUD + worktree/doc wiring | `src/server/routes/tasks.rs`, `src/services/tasks.rs` | `reference/server/routes/tasks.ts` |
| Data model / tables | `src/db/schema.rs`, `src/db/mod.rs` | `reference/server/database/init.sql` |

## Boundaries (not in this spec)

- The workflow flags and the loop that reads them →
  [`orchestration-loop.md`](./orchestration-loop.md).
- How a conversation streams and persists its transcript →
  [`opencode.md`](../extra/harnesses/opencode.md).
- How tasks get authored (board UI, Jira/Notion import) →
  [`kanban-board.md`](../extra/kanban-board.md).
- Opening the PR and merging/cleaning up the worktree →
  [`pull-request-agent.md`](./pull-request-agent.md).
