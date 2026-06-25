# Extra — Auth and multi-user

> **⚠️`omprint` ONLY ⚠️:** Rust convention requires functions and `let` bindings
> use `snake_case` as a naming convention; In all places where `camelCase`
> occurs (referring to the typescript `reference/` implementation of `bottega`),
> substitute for `snake_case` as appropriate; `PascalCase` is used for `trait`s,
> `struct`s, `enum`s, etc

## What it adds

Turns `omprint` from a single-operator tool into something a small company can
deploy: real **user accounts**, **OAuth2/OIDC login** via PKCE, per-user
**API keys** for scripted access, **project-membership authorization** that
scopes every task/project/agent-run to the people allowed to see it, an
**admin** panel for managing users and memberships, and one **role flag**
(`is_technical`) that changes loop behavior for non-technical users (they skip
the manual plan-review gate).

**Scope note up front — this is *app-level* auth: who is allowed to use
`omprint`.** It is **not** the `omp` provider credentials an agent needs to
actually run a turn. Those are a different concern and live in the harness spec
([`harnesses/omp.md`](./harnesses/omp.md)) and in
[`prompt-and-model-customization.md`](./prompt-and-model-customization.md).
The seam: this spec resolves *which user* a request belongs to; those specs
resolve *which model credentials* that user runs agents with.

## Why it's an extra (not core)

Core assumes a single operator on a private box — every request is implicitly
"you," so there is nothing to authenticate or authorize. Multi-user is the
opinionated layer a company adds when more than one person shares a deployment;
skip it and core still works.

## OAuth2/OIDC — how `omprint` authenticates users

`omprint` mandates OAuth2/OIDC for all identity and access management. The
reference implementation's bcrypt password hashing, self-signed JWTs, and
`token_version`-based invalidation are **replaced** by the OAuth2 Authorization
Code Flow with PKCE.

The system supports two modes:

1. **External well-known OIDC endpoint** — `omprint` fetches the provider's
   JWKS from its well-known configuration at startup, caches it, and verifies
   every incoming Bearer token's JWT signature and claims locally. The leptos
   client runs the PKCE flow against the external provider.
2. **Self-hosted rauthy** — `omprint` manages a rauthy instance via PTY
   subprocess, proxied at `/auth` through an axum reverse proxy, with an
   initial admin bootstrap flow.

### High-level flow

1. Browser requests a protected resource; the leptos client detects no session.
2. User clicks "Login" — leptos generates a `code_verifier` and
   `code_challenge` (PKCE), stores the verifier server-side, and redirects to
   the OIDC provider's `authorization_endpoint`.
3. User authenticates at the provider (or is already authenticated via SSO).
4. Provider redirects back to `{base_url}/webapp/auth/callback` with an
   authorization `code`.
5. The server-side SSR handler exchanges the code at the provider's
   `token_endpoint` for an `access_token`, `refresh_token`, and `id_token`.
6. The server validates the ID token (JWKS signature, claims) and
   resolves/creates a local user record keyed to the OIDC `sub` claim.
7. The server stores the `refresh_token` in hiqlite and issues an encrypted,
   httpOnly session cookie to the browser.
8. The browser holds the `access_token` in a JS variable (never localStorage)
   and sends it as `Authorization: Bearer <token>` on every API fetch.
9. When the access token expires (401 response), the client calls
   `/api/auth/refresh`, which uses the server-side refresh token to obtain a
   new access token transparently.

### Session model

- **Refresh tokens** are stored server-side in hiqlite, keyed to the user's
  local `id`. They are never exposed to client-side JS.
- **Session cookies** are encrypted, httpOnly, and secure in production. They
  carry the user's local ID and a session identifier.
- **Access tokens** are short-lived (typ. 5–15 min, controlled by the OIDC
  provider). They are held in a JS variable in the leptos client and sent on
  every API request.
- **Logout** revokes the refresh token at the provider (if the provider
  supports revocation), clears the server-side session, and drops the cookie.

## Configuration — external OIDC provider

When pointing at an external OIDC-compliant identity provider (no rauthy), the
following configuration is required:

| Config key | Required | Description |
|---|---|---|
| `OIDC_ISSUER_URL` | Yes | The issuer URL (e.g. `https://auth.example.com/realms/myorg`). The server fetches `/.well-known/openid-configuration` from this root. |
| `OIDC_CLIENT_ID` | Yes | The OAuth client ID registered with the provider. |
| `OIDC_CLIENT_SECRET` | Yes | The client secret for server-side code exchange at the token endpoint. |
| `OIDC_SCOPES` | No | Space-separated scopes (default: `openid profile email`). |
| `OIDC_ADMIN_CLAIM` | No | A JSON-path claim for auto-detecting admin users on first login (e.g. `realm_access.roles` containing `admin`). If absent, the first user to log in is auto-granted admin. |
| `OM_PRINT_BASE_URL` | Yes | The public base URL of this omprint instance (for constructing the redirect URI sent to the provider: `{base_url}/webapp/auth/callback`). |

### Well-known discovery process

On startup, `omprint` performs OIDC discovery by sending a `GET` request to
`{OIDC_ISSUER_URL}/.well-known/openid-configuration`. The response is a JSON
document conforming to the OIDC Discovery spec, containing at minimum:

- `issuer` — must match `OIDC_ISSUER_URL` (verified)
- `authorization_endpoint` — URL for the PKCE authorization redirect
- `token_endpoint` — URL for the code exchange
- `jwks_uri` — URL for fetching the JWKS key set
- `end_session_endpoint` — optional, for RP-initiated logout
- `response_types_supported` — must include `code`
- `grant_types_supported` — should include `authorization_code` and `refresh_token`

The server parses these endpoints and uses them throughout the OAuth flow. If
the well-known endpoint is unreachable or returns an invalid response, the
server fails loud at startup.

## Configuration — self-hosted rauthy

When no `OIDC_ISSUER_URL` is provided (or `RAUTHY_ENABLED=true` is set),
`omprint` manages its own rauthy OIDC instance:

| Config key | Required | Description |
|---|---|---|
| `RAUTHY_ENABLED` | No | Set `true` to explicitly enable rauthy mode. Inferred from absence of `OIDC_ISSUER_URL`. |
| `RAUTHY_BIN_PATH` | No | Path to rauthy binary. If absent, omprint fetches a known-good release. |
| `RAUTHY_DATA_DIR` | No | Path for rauthy's persistent state (default: `{omprint_config_dir}/rauthy/`). |
| `RAUTHY_ADMIN_EMAIL` | No | Email for the initial bootstrap admin (default: `admin@omprint.local`). |
| `RAUTHY_ADMIN_PASSWORD` | No | Admin password. If absent, omprint auto-generates one and displays it once at first startup (similar to the reference's first-user register flow). |

### PTY lifecycle

- On startup, `omprint` spawns rauthy in a PTY at a random free port.
- `omprint` configures rauthy via environment variables passed to the
  subprocess (rauthy's config surface is mapped from `omprint`'s own config).
- An axum reverse proxy at `/auth` forwards requests to the rauthy port.
- Health check: `GET /auth/health` must return 200 before `omprint` marks
  itself ready.
- On shutdown, `omprint` sends SIGTERM to the rauthy PTY and waits for it to
  exit.
- Crash recovery: if rauthy dies, `omprint` restarts it with exponential
  backoff (1s, 2s, 4s, ... up to a configurable max interval).

### Initial admin bootstrap

If rauthy has no users (first run), the admin credentials from config (or
auto-generated ones) are used:

1. `omprint` creates the initial admin user in rauthy via its admin API.
2. `omprint`'s setup UI shows the admin credentials **once** (like the
   reference's first-user `needsSetup` flow). The password is never stored or
   displayed again.
3. The admin logs into rauthy through the `/auth` proxy, then is redirected
   back to `omprint`'s OAuth callback, where a local user record is created
   with `is_admin = 1`.
4. The admin can then create additional users through `omprint`'s admin panel,
   which maps to rauthy's user management API. Additional users complete the
   OAuth login flow on first access.

## The user model — local table keyed to OIDC `sub`

A `users` table in hiqlite maps the OIDC `sub` claim (the stable identity
reference from the provider) to local attributes. This keeps admin management
and role flags independent of the OIDC provider's claim surface and preserves
the existing project membership and API key patterns.

| Column | Type | Purpose |
|---|---|---|
| `id` | INTEGER PK | Local user ID (used in foreign keys throughout the schema). |
| `oidc_sub` | TEXT UNIQUE | The `sub` claim from the OIDC ID token — the stable identity reference. |
| `username` | TEXT UNIQUE | Display username. Auto-derived from `preferred_username` claim on first login; editable via admin panel. |
| `is_active` | INTEGER DEFAULT 1 | Toggle for disabling accounts without deleting. |
| `is_admin` | INTEGER DEFAULT 0 | Admin flag. Set on first login based on `OIDC_ADMIN_CLAIM` config, or auto-granted to the very first user. |
| `is_technical` | INTEGER DEFAULT 1 | Role flag for auto-advancing past the plan-review gate. |
| `has_completed_onboarding` | INTEGER DEFAULT 0 | Flipped on first provider connection. |
| `git_name` | TEXT | Git author name for commits. |
| `git_email` | TEXT | Git author email for commits. |
| `api_key_hash` | TEXT | SHA-256 of the user's API key (unchanged from reference). |
| `api_key_last_used_at` | TEXT | Timestamp of last API key use. |
| `created_at` | TEXT DEFAULT CURRENT_TIMESTAMP | |
| `last_login` | TEXT | Updated on each OIDC login. |

Key differences from the reference:

- **Removed**: `password_hash` — no local password authentication.
- **Removed**: `token_version` — replaced by OAuth access token expiry and
  refresh token revocation.
- **Removed**: `JWT_SECRET` — no self-signed JWTs; all token verification uses
  the OIDC provider's published JWKS.
- **Added**: `oidc_sub` — the critical new column that links the local user
  record to the OIDC provider's identity.

### First OIDC login flow

1. Extract `sub` from the ID token after successful code exchange.
2. Look up `sub` in `users.oidc_sub`.
3. If not found, INSERT a new row:
   - `oidc_sub = sub`
   - `username = preferred_username` from the ID token claims (fall back to
     `sub` if absent)
   - `is_admin` resolved by `OIDC_ADMIN_CLAIM` config check (or auto-granted
     if this is the very first user)
   - `last_login` set to current timestamp
4. If found, UPDATE `last_login`.
5. Return the local `id` and attributes to set up the session.

The `project_members` join table is unchanged from the reference — it remains
a many-to-many `(project_id, user_id)` join, unique on the pair,
cascade-deleted with either parent.

## Per-user API keys

For scripts, CI, MCP, and `curl` — a long-lived credential that resolves to a
real user (there is **no** global shared key; every API caller has an identity).

- **Generate / regenerate:** `POST /api/account/api-key` mints
  `ccui_` + 32 random bytes (hex), stores **only `sha256(key)`** in
  `users.api_key_hash`, and returns the plaintext **exactly once**. Regenerating
  overwrites the previous hash (one active key per user).
- **Status / revoke:** `GET /api/account/api-key` returns `{ hasKey, lastUsedAt }`
  (never the key); `DELETE /api/account/api-key` nulls the hash.
- **Resolution:** the auth middleware recognizes a token by its `ccui_` prefix
  (`is_api_key_format`), hashes it, and looks up the owning active user
  (`find_user_by_api_key`), best-effort touching `api_key_last_used_at`. API keys
  are **not** eligible for refresh (they are already long-lived).

> API keys bypass the OAuth flow entirely — they are a second credential type
> recognized by the auth middleware's prefix check. This is important for
> scripted access, CI/CD pipelines, and WebSocket connections where PKCE is
> impractical.

See the reference implementation: `reference/server/services/userApiKey.ts`
(`generateApiKey`, `findUserByApiKey`, `isApiKeyFormat`, `getApiKeyStatus`,
`revokeApiKey`) and `reference/server/routes/account.ts`.

**FIXME:** Replace references above with links to the `omprint` Rust
implementation once it exists.

## The auth middleware — JWKS-based token verification

A Tower middleware in axum handles all request authentication. It replaces the
reference's JWT+bcrypt middleware with JWKS-based OAuth token verification.

**Startup:**
1. OIDC discovery: fetch `{issuer}/.well-known/openid-configuration`.
2. Extract `jwks_uri` from the discovery document.
3. Fetch the JWKS (JSON Web Key Set) from `jwks_uri`.
4. Cache the key set in memory, indexed by `kid` (key ID).
5. If the well-known endpoint is unreachable or the JWKS is invalid, the
   server fails loud and refuses to start.

**Per-request (`authenticate_token`):**
1. Extract the token from the `Authorization: Bearer <token>` header (or from
   the `?token=` query parameter for WebSocket / compatibility clients).
2. Check the token prefix:
   - If it starts with `ccui_`, skip JWKS verification, hash the key, and
     resolve the user via `api_key_hash` lookup.
   - Otherwise, treat it as an OAuth access token (JWT).
3. For JWT tokens: decode the JWT header to find the `kid`. Look up the
   corresponding key in the cached JWKS. If not found, trigger a JWKS refresh
   (see "Key rotation"). Verify the signature using `jsonwebtoken` crate with
   the matching key.
4. Validate standard claims:
   - `iss` (issuer) must match the configured `OIDC_ISSUER_URL`.
   - `exp` (expiration) must be in the future.
   - `aud` (audience) must include the configured `OIDC_CLIENT_ID`.
5. Extract `sub` from the validated token and look up the local user by
   `oidc_sub`. Attach the resolved user to the request extensions.
6. If any verification step fails (invalid signature, expired token, missing
   user, etc.), return 401 Unauthorized.

**Key rotation:**
- If a token's `kid` is not present in the cached JWKS, fetch a fresh JWKS
  from the provider and re-attempt verification.
- A periodic background refresh re-fetches the JWKS every hour.
- On `exp` or `nbf` claim mismatch, reject immediately (no refresh retry).

**`require_admin`:**
A second Tower layer that runs after `authenticate_token`. It checks
`req.user.is_admin`. If false (or no user), returns 403 Forbidden. Applied to
all `/api/admin/*` routes.

**WebSocket authentication:**
The same `resolve_token` logic applies during the WebSocket upgrade handshake,
using the `?token=` query parameter. The middleware extracts the token from the
WebSocket request URI, verifies it (JWKS or API key), and attaches the user to
the connection context. Per-message authorization (for `claude-command`, abort,
resume) re-checks project membership.

**No localhost bypass:**
The reference's narrow localhost bypass (falling back to the first user for one
internal endpoint) is omitted. All requests, regardless of origin, go through
the same authentication path.

## Client-side PKCE flow — leptos

The leptos web application implements the OAuth2 Authorization Code Flow with
PKCE (Proof Key for Code Exchange), initiated from the server-side rendered
page.

### Login initiation

1. Unauthenticated users are redirected to the login page (`/webapp/login`).
2. The "Login" button triggers PKCE setup:
   - Generate a `code_verifier` — 43 random URL-safe characters.
   - Compute the `code_challenge` — SHA-256 hash of the verifier, base64url
     encoded.
   - Store the `code_verifier` in a short-lived server-side session (or
     encrypted cookie).
3. Redirect the browser to the provider's `authorization_endpoint` with:
   ```
   ?response_type=code
   &client_id={client_id}
   &redirect_uri={base_url}/webapp/auth/callback
   &code_challenge={challenge}
   &code_challenge_method=S256
   &scope=openid+profile+email
   &state={anti-forgery-state}
   ```

### Callback handling

The callback route (`GET /webapp/auth/callback`) is an SSR endpoint that:

1. Extracts `code` and `state` from query parameters. Validates `state` against
   the stored value (anti-forgery).
2. POSTs to the provider's `token_endpoint` with:
   ```
   grant_type=authorization_code
   &code={code}
   &redirect_uri={base_url}/webapp/auth/callback
   &client_id={client_id}
   &client_secret={client_secret}
   &code_verifier={verifier}
   ```
3. Receives `access_token`, `refresh_token`, and `id_token`.
4. Validates the ID token using the same JWKS verification as the server
   middleware (signature, `iss`, `aud`, `exp`).
5. Extracts `sub` and `preferred_username` from the ID token claims.
6. Resolves or creates the local user record (see "First OIDC login flow").
7. Stores the `refresh_token` server-side in hiqlite, keyed to the user's
   local `id`.
8. Sets an encrypted, httpOnly session cookie containing the user's local `id`.
9. Stores the `access_token` in the SSR response context so the leptos client
   can capture it as a JS variable.
10. Redirects the browser to the app root (`/webapp`).

### Subsequent requests

- The leptos client holds the access token in a Rust/JS variable (in-memory
  only, never persisted to localStorage or sessionStorage).
- Every API fetch includes `Authorization: Bearer <token>`.
- When the server returns 401 Unauthorized (token expired), the client calls
  `POST /api/auth/refresh` (authenticated by the session cookie). The server
  uses the stored refresh token to obtain a new access token from the provider,
  stores the new refresh token, and returns the new access token.
- If the refresh fails (refresh token expired or revoked), the user is
  redirected to the login page.

This mirrors the reference's `X-Refreshed-Token` rolling refresh pattern but
uses OAuth semantics — access tokens are short-lived by nature, and the
server-side refresh token is the long-lived credential.

## OAuth endpoints on the API

The auth route table is updated to reflect OAuth-based authentication:

| Route | Method | Auth | Purpose |
|---|---|---|---|
| `/api/auth/status` | GET | None | Returns `{ needs_setup (if zero users), is_authenticated, user }` — driven by OAuth session cookie. |
| `/api/auth/login` | GET | None | Initiates the PKCE flow: generates code_verifier, stores it, returns the authorization URL. Client redirects the browser. |
| `/api/auth/callback` | GET | None | The OAuth callback: exchanges code for tokens, resolves/creates user, sets session cookie, redirects to app. |
| `/api/auth/refresh` | POST | Session cookie | Uses server-side refresh token to obtain a new access token. Returns the new access token. |
| `/api/auth/user` | GET | authenticate_token | Returns current user (unchanged). |
| `/api/auth/logout` | POST | authenticate_token | Revokes refresh token at the provider (if supported), clears local session, clears cookie. |
| `/api/auth/profile` | PUT | authenticate_token | Toggle `is_technical` (unchanged). |

Routes **removed** from the reference:
- `POST /api/auth/register` — no local registration; user provisioning is
  handled through the OIDC provider.
- `POST /api/auth/login` — no local password login; authentication is handled
  entirely through OIDC.

Routes **unchanged** from the reference:
- `GET /api/auth/user` — returns the safe user record for the current session.
- `PUT /api/auth/profile` — toggles `is_technical` for the current user.

## Admin panel and user management (adapted)

Admins are users with `is_admin = 1`. The first user to log in is auto-granted
admin (unless an `OIDC_ADMIN_CLAIM` configuration defers this to the provider).
An admin can grant admin status to others through the admin panel. All admin
routes mount behind `authenticate_token, require_admin` (`/api/admin/*`).

- **User management** (`/api/admin/users`): list all users, create a user
  (pre-create a local record keyed to an OIDC `sub`), update (rename,
  toggle `is_active`/`is_admin`), and delete — **with the guard that an admin
  cannot delete their own account**.
  - For **rauthy**: admin creation calls rauthy's admin API to create the user
    at the provider; the user then completes OAuth login on first access.
  - For **external OIDC**: user creation is handled at the provider; the admin
    pre-creates a local user record with the known `oidc_sub`. On first OIDC
    login, the existing record is matched and used.
  - No password fields in create/update — passwords are managed by the OIDC
    provider.
- **Delete cleanup**: when deleting a local user record, if rauthy is used,
  the corresponding provider-side user is also deleted via rauthy's admin API.
- **Project membership** (`/api/admin/projects`, `/projects/:id/members`): list
  projects with member counts, add/remove members, with the guard that the
  **last member of a project cannot be removed** (so no project is orphaned).
  Unchanged from the reference.
- **UI:** a two-tab page (Users / Project Memberships) — unchanged from the
  reference, except the user create/edit form omits password fields.

Responses use a **safe user shape** — `api_key_hash` is never serialized out of
any endpoint.

## Project-membership authorization

This section is preserved from the reference with minimal changes. The single
chokepoint is `has_project_access(project_id, user_id)` — it returns true if
the user is an admin **or** a member of the project. The companion helpers
(`get_all_projects`, `get_project`, `update_project`, `delete_project`) take
the admin-vs-member fork once so callers don't repeat it.

What this means concretely:

- **Project creation registers the owner as a member atomically.** Insert the
  project row and the `project_members` row in one transaction. Forget this and
  the creator instantly loses access to their own project.
- **List endpoints are filtered by membership**, not "all rows": queries JOIN
  through `project_members` on the requesting `user_id`. Admins get the
  unfiltered admin set.
- **Every task/agent-run route re-checks `has_project_access`** after resolving
  the task's project, returning 403 (or 404) on failure.
- **WebSocket actions are authorized the same way.** Before acting on a
  `claude-command` / abort / resume, the dispatcher walks
  conversation → task → project and calls `has_project_access`; unauthorized
  actions are dropped with a log line.

## The `is_technical` role and its one behavioral effect

`is_technical` (default 1) is the only role flag that changes the orchestration
loop, and it does exactly one thing: **non-technical users skip the
human-review gate after planning.** Core always stops after the planning agent
so a human can read the plan and press Run for implementation
([`../core/orchestration-loop.md`](../core/orchestration-loop.md) defers this
exception here). For a non-technical user that gate is friction, so:

1. **Auto-advance.** When a planning (`planification`) run completes, the
   chaining handler checks the **acting user's** `is_technical`. If they are
   non-technical (`is_technical === 0`) — and the task isn't blocked or at the
   iteration cap — it auto-starts the implementation agent (on the same ~1s
   settle delay as every other chained start). Technical users keep the manual
   gate. The decision tracks the user who *triggered planning* (carried on the
   streaming context), falling back to the task owner only when context has no
   user.
2. **Prompt variant.** A non-technical run also uses a different planning
   prompt (`planification-nontechnical` instead of `planification`), selected
   by the same `is_technical` resolution at run start.

A user toggles their own `is_technical` via `PUT /api/auth/profile`

## Rate limiting on auth routes

The unauthenticated `/api/auth/login` (GET) and `/api/auth/status` routes carry
a per-IP throttle to blunt abuse. The implementation uses axum-compatible rate
limiting (e.g., `tower_governor`) with a generous window/max
(env-tunable via `LOGIN_RATE_LIMIT_MAX` / `LOGIN_RATE_LIMIT_WINDOW_MIN`) and
`skip_successful_requests: true`, so a successful login never eats the budget.

The principle is the same as the reference's `express-rate-limit` on the login
and register routes — but adapted to axum middleware and scoped to the
OAuth-specific endpoints that exist in this spec.

## WebSocket authentication

WebSocket connections authenticate using the same `?token=` query parameter
pattern as the reference. The token is now an OAuth access token (verified
against JWKS) or an API key (prefix-routed to hash lookup). The same
`resolve_token` logic from the Tower middleware is reused during the WebSocket
upgrade handshake.

Per-message authorization (`authorize_conversation_access`,
`authorize_session_access`) walks conversation → task → project and calls
`has_project_access` — unchanged in logic from the reference.

## Onboarding flags

- **`has_completed_onboarding`** — flipped to 1 not by a dedicated route but as
  a side effect of the user **connecting their first agent provider** (the
  app's real first-use milestone). See `complete_onboarding` /
  `has_completed_onboarding` called from the agent-model-settings service. The
  provider-connect flow and the first-login modal belong to
  [`prompt-and-model-customization.md`](./prompt-and-model-customization.md);
  this spec only owns the flag and its column.
- `is_technical` doubles as an onboarding signal — a fresh non-technical user
  starts auto-advancing past the plan gate immediately.

## What becomes user-scoped

When you layer this on core, these stop being implicitly "the operator's" and
become per-user / membership-gated:

- **All `/api/*` routes** mount behind `authenticate_token`; `req.user` is
  always present downstream.
- **Projects** are listed/read/written through membership (`has_project_access`);
  creation auto-members the creator.
- **Tasks, agent runs, conversations** inherit their project's membership — every
  handler resolves the task's project and re-checks access.
- **WebSocket commands** (chat, abort, resume) re-check access per message.
- **Agent runs and prompts** resolve the *acting user* (for `is_technical` and,
  via the model-customization extra, for per-user provider credentials).
- **`/api/admin/*`** additionally requires `require_admin`.

## What to build

- [ ] `users` columns: `oidc_sub`, `is_active`, `is_admin`, `is_technical`,
      `has_completed_onboarding`, `api_key_hash`, `api_key_last_used_at`;
      no `password_hash`, no `token_version`; the `project_members` join table;
      `user_id` owner columns on `projects`/`tasks`.
- [ ] OIDC discovery: fetch `{issuer}/.well-known/openid-configuration` on
      startup, parse endpoints, fail loud on failure.
- [ ] JWKS fetching and caching: fetch key set from `jwks_uri`, cache by `kid`,
      refresh on unknown `kid` or hourly background interval.
- [ ] Tower middleware: `authenticate_token` (JWKS + API key prefix routing),
      `require_admin`; matching WebSocket auth via `?token=`.
- [ ] Session management: store refresh tokens in hiqlite, issue encrypted
      httpOnly session cookies, token refresh (`/api/auth/refresh`), logout
      (revoke + clear).
- [ ] PKCE flow initiation: `GET /api/auth/login` returns authorization URL;
      code verifier/challenge generation.
- [ ] OAuth callback: `GET /api/auth/callback` — SSR endpoint that exchanges
      code for tokens, validates ID token, resolves/creates local user, sets
      session cookie.
- [ ] Per-user API keys: `ccui_` plaintext shown once, `sha256` stored,
      generate/status/revoke routes, prefix-based resolution in the middleware.
- [ ] `has_project_access` + the admin/member fork helpers, applied to every
      project/task/agent-run/conversation route **and** the WS dispatcher.
- [ ] Atomic owner-membership on project create; membership-filtered list
      queries.
- [ ] Admin routes: user CRUD (no password fields, self-delete guard) + project
      membership (last-member guard); rauthy provider-side user management.
- [ ] Rauthy PTY lifecycle: spawn, health check, reverse proxy at `/auth`,
      SIGTERM on shutdown, exponential backoff restart.
- [ ] Rauthy admin bootstrap: one-time credential display, initial user creation
      via rauthy admin API.
- [ ] The `is_technical` auto-advance after planning + the non-technical planning
      prompt variant (unchanged from reference).
- [ ] Per-IP rate limit on unauthenticated auth endpoints (e.g., `tower_governor`).

## Reference map

| Concern | Location |
|---|---|
| OIDC discovery, JWKS fetch/cache/refresh | FIXME: (none yet — this spec is being written before the Rust implementation exists) |
| `authenticate_token` / `require_admin` Tower middleware | FIXME: (none yet) |
| Session management (refresh token store, session cookie) | FIXME: (none yet) |
| PKCE flow initiation and callback SSR handler | FIXME: (none yet) |
| Token refresh (`/api/auth/refresh`) | FIXME: (none yet) |
| Logout (token revocation, session clear) | FIXME: (none yet) |
| API-key mint/hash/resolve | `reference/server/services/userApiKey.ts` (retained from reference) |
| `has_project_access` + access-scoped project helpers | `reference/server/services/projectService.ts` (retained from reference) |
| Schema (users, project_members, oidc_sub, api_key_hash) | FIXME: (none yet) |
| Owner-membership on create, membership-filtered queries | `reference/server/database/db.ts` (retained from reference) |
| Non-technical auto-advance after planning | `reference/server/services/conversation/agentRunLifecycle.ts` (retained from reference) |
| Non-technical planning prompt selection | `reference/server/constants/agentPrompts.ts`, `reference/server/services/agentRunner.ts` (retained from reference) |
| WebSocket per-action access checks | `reference/server/websocket/dispatch.ts` (retained from reference) |
| Rauthy PTY lifecycle and reverse proxy | FIXME: (none yet) |
| Admin UI | `reference/src/pages/AdminPage.tsx`, `reference/src/components/Admin/` (retained from reference) |

## Boundaries (not in this spec)

- **`omp` provider credentials** — how a resolved user's configured `models.yml`
  authenticates an agent turn → [`harnesses/omp.md`](./harnesses/omp.md).
- **Per-user provider/model selection**, the provider-connect flow, and the
  first-login provider modal that flips `has_completed_onboarding` →
  [`prompt-and-model-customization.md`](./prompt-and-model-customization.md).
- The loop itself — chaining, the iteration cap, the plan gate this extra
  bypasses → [`../core/orchestration-loop.md`](../core/orchestration-loop.md).
- Project/task/worktree mechanics this layer authorizes →
  [`../core/task-and-workspace.md`](../core/task-and-workspace.md).
