# Architecture

## Project Layout

```
omprint/
├── Cargo.toml          # Workspace root with omprint binary crate
├── src/
│   ├── main.rs         # Entry point: DB init → migrations
│   ├── lib.rs          # Re-exports db module for integration tests
│   └── db/
│       ├── mod.rs      # Migration SQL constants and run_migrations()
│       └── schema.rs   # Domain model structs and enums
├── tests/
│   └── migration_test.rs  # Integration tests for migrations
├── README.md           # Project overview
└── ARCHITECTURE.md     # This file
```

The workspace has a single member crate (`omprint` binary) defined inline. Future tasks may add separate member crates (e.g., `omprint-core`, `omprint-axum`).

## Database

- **Engine**: [hiqlite](https://crates.io/crates/hiqlite) — async, Raft-capable embedded SQLite with built-in durability via WAL + auto-heal crash recovery. Single-node deployment eliminates the Mutex bottleneck in axum handlers.
- **Schema**: 11 tables defined via raw SQL DDL in `src/db/mod.rs`. UUIDs are stored as `TEXT`, booleans as `INTEGER` (0/1), JSON as `TEXT`, and timestamps as ISO 8601 `TEXT` strings.
- **Migration system**: A `_migrations` tracking table records which migrations have been applied. Each migration is a named SQL DDL statement; only unapplied migrations execute on startup.

### Tables

| Table | Purpose |
|-------|---------|
| `users` | User accounts with OIDC auth |
| `projects` | Project definitions (repo paths, monorepo subproject paths) |
| `project_members` | Many-to-many user/project join table |
| `tasks` | Task definitions with workflow state flags |
| `conversations` | LLM conversation sessions (omp-mediated) |
| `task_agent_runs` | Agent execution tracking per task |
| `messages` | Transcript events (composite PK: project_key, session_id, seq) |
| `session_summaries` | Session memory snapshots (composite PK: project_key, session_id) |
| `app_settings` | Global key-value configuration store |
| `user_agent_model_settings` | Per-user agent/model configuration |

## Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| tokio | 1 (full) | Async runtime |
| axum | 0.8 | Web framework (future use) |
| hiqlite | 0.13 | Async embedded SQLite (Raft-capable, WAL + auto-heal) |
| serde | 1 (derive) | Serialization/deserialization |
| serde_json | 1 | JSON support |
| uuid | 1 (v4) | UUID generation |
| chrono | 0.4 (serde) | Timestamp types |

## Application Lifecycle

1. **Startup**: Start hiqlite node with `data_dir`, auto-heal enables WAL + foreign keys internally
2. **Migrations**: Run `db::run_migrations()` — applies any pending DDL
3. **Future**: Server startup (axum), subprocess management (omp supervision)

## Recurring Patterns

- **snake_case** naming for all columns and Rust identifiers
- **`Result<T, Box<dyn std::error::Error>>`** error handling throughout
- **`TEXT` storage** for UUIDs, timestamps, and JSON values (SQLite convention)

## Design Decisions

- **hiqlite over rusqlite**: hiqlite provides an async, Raft-capable SQLite database with built-in durability via WAL + auto-heal crash recovery. Single-node deployment eliminates the Mutex bottleneck in axum handlers.
- **OIDC over password auth**: Production-ready authentication without implementing bespoke password handling. Supports enterprise SSO.
- **Embedded DB over client-server**: Eliminates external database infrastructure for development and small-scale deployments. hiqlite manages state files inside the configured `data_dir`.
- **Raw SQL DDL over migration framework**: DDL is wrapped in a simple `_migrations` tracking table, keeping the migration system self-contained.
