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
  chat or an agent run. See [`omp-integration.md`](./omp-integration.md).

Schema: `src/db/schema.rs` (Rust domain model) and `src/db/mod.rs` (13 DDL migrations). See also the reference [`reference/server/database/init.sql`](../reference/server/database/init.sql).

Task `status` moves `pending → in_progress → in_review → completed`. The loop
flips `pending → in_progress` on the first agent activity (see
[`orchestration-loop.md`](./orchestration-loop.md)).

### Integer- vs UUID-based IDs

The database uses **UUIDs** for all primary keys (`Task.id`, `Project.id`). The
archive path pattern `task-{taskId}.md` accepts a string ID (e.g. `"42"` or a
UUID) — the `sanitize_id` function in `src/archive/paths.rs` rejects path
traversal but is otherwise opaque to the ID format.

However, `src/worktree/mod.rs` uses **integer IDs** (`u32`) for worktree paths
and branch naming — e.g. `project-1/task-42/` and `task/42-foo-bar`. The
`uuid_to_u32` function XOR-folds a UUID's 128 bits into a `u32` to derive the
integer used in filesystem paths. This means:
- **Archive paths** use the original ID string (UUID or integer) directly
- **Worktree paths** (`src/worktree/mod.rs`) and branch names always use folded
  `u32` integers for brevity in filesystem and git operations

## The markdown document — the source of truth for "what to build"

- **Location:** a central, per-user archive **outside the repo** —
  `~/.ofm/projects/{projectId}/tasks/task-{taskId}.md` (root overridable via
  `ofm_ARCHIVE_ROOT`).
- **Why outside the repo (the load-bearing decision):** the doc must survive the
  worktree being torn down when the task's PR merges. If it lived inside the
  worktree it would vanish with it. Keeping it in a separate archive means the
  plan, the to-do checklist, and the review history outlive any single
   worktree. See `get_archive_root` / `get_task_doc_path` in [`src/archive/paths.rs`](../src/archive/paths.rs) and `ArchiveRoot` in [`src/archive/mod.rs`](../src/archive/mod.rs).
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
  in [`src/archive/mod.rs`](../src/archive/mod.rs).

## The worktree — the isolated workspace

- **One git worktree per task**, at `{repo_folder_path}-worktrees/task-{taskId}/`
  — a sibling directory, never inside the repo itself.
- **Branch:** `task/{taskId}-{sanitized-title}`, cut from the repo's default
  branch (resolved via `origin/HEAD`, falling back to `main`/`master`).
- **Why a worktree, not a checkout:** every task gets a real, independent working
  directory, so concurrent tasks never collide on the filesystem and the user's
  main checkout is never disturbed.
- **Created at task creation** when the project path is a git repo; if worktree
   creation fails, the task row is rolled back (see the create handler in
   [`src/server/routes/tasks.rs`](../src/server/routes/tasks.rs) (`create_task` handler, includes worktree creation with rollback)).
- **Create-time conveniences** so an agent can build and test immediately:
  symlink the repo's `.env*` files into the worktree, create gitignored dirs,
   and copy `node_modules` / `.venv` in the background. See `create_worktree` in
   [`src/worktree/mod.rs`](../src/worktree/mod.rs) (branch naming, default-branch detection, env symlinks, gitignored dirs, dependency copy).
  - **NOTE**: Windows may require copying files, because its support for
  symlinks (and user creation/management) is conditional on system policies
  - **`ofm` ONLY:** On a per-project basis allow the User to configure
  zero-or-more additional files to copy/symlink from the repo to the worktree,
  as above
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

## How the document becomes agent context

When an agent run starts, the orchestrator assembles a context system-prompt
from the archive (`build_context_prompt` in `src/archive/mod.rs`). It:

- names the authoritative task-doc path and instructs the agent to **read it in
  full first**,
- lists any input files to read for additional context,
- includes the testing configuration (task id, the assigned dev-server port,
  and test-execution best practices).

The agent then reads and edits the doc directly with its own file tools. The doc
path in the prompt is authoritative — agents are told not to look elsewhere.

## What to build

- [x] `projects` / `tasks` / `conversations` tables → implemented in `src/db/schema.rs`
- [x] Configurable archive root → `src/archive/paths.rs`, `src/config.rs`
- [x] Doc read/write/delete + archive cleanup → `src/archive/mod.rs`
- [x] Worktree create/remove/status → `src/worktree/mod.rs`
- [x] Task create with rollback and doc seeding → `src/server/routes/tasks.rs`
- [x] Task delete with worktree/archive cleanup → `src/server/routes/tasks.rs`
- [x] `buildContextPrompt` → `build_context_prompt` in `src/archive/mod.rs`
- [x] Dev-server port assignment via `get_dev_server_port` → `src/archive/mod.rs`
- [ ] Effective-cwd resolution (not yet wired into agent runs)

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
  [`omp-integration.md`](./omp-integration.md).
- How tasks get authored (board UI, Jira/Notion import) →
  [`kanban-board.md`](../extra/kanban-board.md).
- Opening the PR and merging/cleaning up the worktree →
  [`pull-request-agent.md`](./pull-request-agent.md).
