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
   `spawn_reader` in `src/providers/oh_my_pi/mod.rs` for a concrete example.

## UI Conventions

- Content containers use Bulma `.box` for block-level content, `.card` for sub-units / grid items (e.g., kanban boards).
- Icons use MDI via `@mdi/font` CDN, applied with Bulma's `.icon` wrapper pattern.

## Environment Variables

All env vars use the `OFM_` prefix. Key ones:

| Variable | Default | Description |
|---|---|---|
| `OFM_PORT` | `3183` | HTTP listen port |
| `OFM_HOSTNAME` | `127.0.0.1` | HTTP listen hostname |
| `OFM_FOOTPRINT` | `~/.ofm` | Per-user data directory (archive, config, DB) |
| `OFM_RAUTHY_ENABLED` | `false` | Enable local rauthy OIDC provider |
| `OFM_RAUTHY_PORT` | `0` (random) | Port for rauthy instance (0 = random) |
| `OFM_OIDC_ISSUER_URL` | unset | OIDC issuer URL for external auth |
| `OFM_OIDC_CLIENT_ID` | unset | OIDC client ID |
| `OFM_API_KEY` | unset | API key for machine access |

## Playwright CLI Setup (one-time, per user)

For end-to-end browser testing with Playwright (no MCP, just CLI that is agent-friendly):

```bash
# 1. Install the CLI tool globally (idempotent — npm will skip if already present)
npm install -g @playwright/cli@latest

# 2. Verify the binary is on PATH
which playwright-cli

# 3. Install Chromium browser (idempotent — ignores if already installed)
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

Use `playwright-cli --help` to explore the CLI's capabilities

## `ofm` + Rauthy for isolated, local testing

The project includes built-in rauthy lifecycle management (spawn/cleanup).
To start an isolated server for end-to-end testing:

```bash
# Pick a random port for ofm (avoid conflicts with other worktrees)
OFM_PORT=3205 \
  OFM_FOOTPRINT="$PWD/.ofm" \
  OFM_RAUTHY_ENABLED=true \
  cargo run
```

On first run, ofm will:
1. Download and start rauthy in the footprint directory
2. Print the admin credentials — note these down
   - **Username:** `admin@localhost`
   - **Password:** printed in the startup logs (search for "admin password")
3. Serve the webapp at `http://localhost:3205`
4. All data lives under `$PWD/.ofm` — deleting the worktree cleans it up

The isolated footprint (`$PWD/.ofm`) prevents interference between worktrees.
The `.ofm` directory is gitignored so it won't accidentally be committed.

**If you forget the admin password**, check the rauthy config file:
`$PWD/.ofm/rauthy/rauthy.cfg` — the hash is in there, but you can also
delete `$PWD/.ofm/rauthy` and restart to trigger a fresh install with a
new password.

> 💡**Resetting `ofm`**: The `.ofm` footprint can be deleted then recreated (by
> restarting `ofm`) between testing phases, if a reset of `ofm` state is desired,
> or if the admin password is lost.

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
```

## Documentation Updates

Every task implementation **must**:

1. **Update relevant spec files** if the implementation changes behavior described in `spec/SPEC.md` or any `spec/core/*.md` / `spec/extra/*.md` file.
2. **Update ARCHITECTURE.md** if new modules are added, module responsibilities change, or dependencies change.
3. **Update README.md** if user-facing behavior changes (ports, env vars, auth, setup).
4. **Update existing `src/` citations** in spec files if line numbers or file paths change.
5. **Reference the updated doc files** in the task output or PR description.
6. **Leave a `FIXME` comment** in the doc for the next human pass if a citation needs updating but the implementation agent cannot verify correctness (e.g., line numbers).

Documentation drift is unacceptable. If the implementation changes something the spec describes, update the spec in the same task.
