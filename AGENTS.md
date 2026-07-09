# OMP Subprocess Integration Notes for AI Agents

## Threading

- **Avoid `std::thread::spawn`.** This project runs inside a tokio runtime; spawning
  raw OS threads bypasses tokio's scheduling and can cause issues with resource
  tracking, test flakiness, and runtime shutdown.
- Prefer `tokio::spawn` for lightweight async tasks that run on the tokio
  runtime's worker threads.
- When blocking I/O is unavoidable (e.g. reading from a PTY), use
  `tokio::task::spawn_blocking`. The blocking task reads from the I/O source
  and sends events through an `mpsc::Sender` via `blocking_send`. See
  `spawn_reader` in `src/omp/mod.rs` for a concrete example.

## UI Conventions

- Content containers use Bulma `.box` for block-level content, `.card` for sub-units / grid items (e.g., kanban boards).
- Icons use MDI via `@mdi/font` CDN, applied with Bulma's `.icon` wrapper pattern.

## Environment Variables

All env vars use the `OMPRINT_` prefix. Key ones:

| Variable | Default | Description |
|---|---|---|
| `OMPRINT_PORT` | `3183` | HTTP listen port |
| `OMPRINT_HOSTNAME` | `127.0.0.1` | HTTP listen hostname |
| `OMPRINT_FOOTPRINT` | `~/.omprint` | Per-user data directory (archive, config, DB) |
| `OMPRINT_RAUTHY_ENABLED` | `false` | Enable local rauthy OIDC provider |
| `OMPRINT_RAUTHY_PORT` | `0` (random) | Port for rauthy instance (0 = random) |
| `OMPRINT_OIDC_ISSUER_URL` | unset | OIDC issuer URL for external auth |
| `OMPRINT_OIDC_CLIENT_ID` | unset | OIDC client ID |
| `OMPRINT_API_KEY` | unset | API key for machine access |
| `OMPRINT_FOOTPRINT` | `~/.omprint` | Per-user data directory |

## Playwright CLI Setup (one-time, per user)

For end-to-end browser testing with Playwright MCP:

```bash
# 1. Install the CLI tool globally (idempotent — npm will skip if already present)
npm install -g @playwright/cli@latest

# 2. Verify the binary is on PATH
which playwright-cli

# 3. Verify the binary is on PATH
which playwright-cli

# 4. Install Chromium browser (idempotent — ignores if already installed)
npx playwright install chromium
```

The `@playwright/cli` package also brings Playwright's browser installer, so
`npx playwright install chromium` (or `firefox` / `webkit`) works.

### Using playwright-cli

Always specify `--browser=chromium` when opening a browser session (the default
`chrome` channel requires a system-installed Google Chrome):

```bash
export PATH="$HOME/.npm-global/bin:$PATH"
export PLAYWRIGHT_BROWSERS_PATH="$HOME/.cache/ms-playwright"

playwright-cli open --browser=chromium http://localhost:3205
playwright-cli snapshot
playwright-cli close
```

## Rauthy — Local OIDC for Isolated Testing

The project includes built-in rauthy lifecycle management (spawn/proxy/cleanup).
To start an isolated server for end-to-end testing:

```bash
# Pick a random port for omprint (avoid conflicts with other worktrees)
OMPRINT_PORT=3205 \
  OMPRINT_FOOTPRINT="$PWD/.omprint" \
  OMPRINT_RAUTHY_ENABLED=true \
  cargo run
```

On first run, omprint will:
1. Download and start rauthy in the footprint directory
2. Print the admin credentials — note these down
   - **Username:** `admin@localhost`
   - **Password:** printed in the startup logs (search for "admin password")
3. Serve the webapp at `http://localhost:3205`
4. All data lives under `$PWD/.omprint` — deleting the worktree cleans it up

The isolated footprint (`$PWD/.omprint`) prevents interference between worktrees.
The `.omprint` directory is gitignored so it won't accidentally be committed.

**If you forget the admin password**, check the rauthy config file:
`$PWD/.omprint/rauthy/rauthy.cfg` — the hash is in there, but you can also
delete `$PWD/.omprint/rauthy` and restart to trigger a fresh install with a
new password.

## Unit / Integration Tests

```bash
# Run all tests
cargo test --lib --tests

# Run specific unit tests
cargo test --lib -- markdown_viewer
cargo test --lib -- project_card
cargo test --lib -- board

# Run integration tests (brings up in-process server)
cargo test --tests -- webapp_test
