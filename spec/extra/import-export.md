# Extra — Import / Export Settings Tabs

> **Implementation status:** ✅ Implemented (`src/services/export_import.rs`, `src/server/routes/settings.rs`, `src/webapp/pages/settings.rs`)

## What it adds

Two new tabs in the Settings page — **Export** and **Import** — that let users
download project+task data as JSON and re-import it into the same or a
different OFM instance.

## Export

The Export tab lists all projects with checkboxes. The user selects which
projects to include and clicks "Export Selected". A JSON file is downloaded
with the following structure:

- `exported_at` — ISO 8601 timestamp of export
- `exported_by` — authenticated user's id and username
- `projects` — array of project objects, each containing:
  - `id`, `name`, `repo_folder_path`, `subproject_path`, `created_at`
  - `tasks` — array of task objects, each containing:
    - `id`, `project_id`, `title`, `status`, `description`, `created_at`
    - `conversations` — array of conversation metadata objects:
      - `id`, `provider_session_id`, `model`, `effort`, `name`, `created_at`, `updated_at`

**Description source:** The `description` field is read from the task's
markdown document file on disk via `ArchiveRoot::read_task_doc`.

**No messages exported:** Conversation metadata is included but no `messages`
rows are exported.

## Import

The Import tab provides a file upload form for JSON files. The import flow
has two phases:

1. **Preview** (`POST /api/settings/import/preview`): Parses the uploaded JSON,
   validates structure, and returns a preview with per-project cards showing
   task counts and any validation errors. Unknown fields in the JSON are
   silently ignored for schema flexibility.

2. **Execute** (`POST /api/settings/import/execute`): Accepts the user's
   mapping decisions and performs the inserts.

Each project in the preview shows:

- An **enable/disable checkbox** (default: enabled). Disabled projects are
  displayed but read-only.
- An **import target dropdown**:
  - "Create new project" — shows editable name and repository path fields,
    pre-filled from the source JSON
  - "Add to existing project" — shows a dropdown of existing projects
- A **task count badge**

**ID handling:** Source primary keys are received but NOT reproduced. New IDs
are generated via `COALESCE(MAX(id),0)+1` for projects and tasks.
Conversations receive fresh `Uuid::new_v4()`.

**Conversations:** Conversation metadata is imported (model, effort, name,
timestamps) with new UUIDs. No `messages` rows are created.

**Error handling:** Parse failures show a `.notification.is-danger` box with
the error message. On success, the page redirects to the All Projects page.

## API

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/api/settings/export?project_ids=1,2,3` | Bearer/API key | Export projects as JSON |
| POST | `/api/settings/import/preview` | Bearer/API key | Parse and validate import JSON |
| POST | `/api/settings/import/execute` | Bearer/API key | Execute import with mapping decisions |

## Body limit

Import routes accept bodies up to 10MB (via per-route `DefaultBodyLimit`).
