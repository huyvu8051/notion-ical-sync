# CalDAV auth & routing

How the server figures out *whose* calendar to serve, given that CalDAV
clients (Apple Calendar, etc.) only ever ask for three things: **Server**,
**Username**, **Password** — no path field, no tenant/database id.

## The short answer

There is no per-user subdomain or URL prefix. Every tenant hits the same
bare host (`notion-caldav.opendiy.vn`). The server learns which
user/calendar to serve entirely from the **HTTP Basic Auth identity**
resolved during a standard discovery handshake (RFC 6764). The client is
expected to do this handshake automatically the moment you give it a bare
hostname — that's the whole point of the protocol, and it's why the old
single-tenant deployment (`notion-sync.opendiy.vn`, one calendar, one
password) also "just worked" with only Server/Username/Password: the
discovery chain was already running, it just always resolved to the one
calendar that existed.

## The discovery chain

1. **`PROPFIND /.well-known/caldav`** (or the client tries this first,
   unauthenticated) → server responds `401` with `WWW-Authenticate: Basic`.
   Client retries with the Basic Auth credentials the user typed in.
2. With credentials attached, the same request is now authenticated by
   `auth_middleware` (looks up `calendars.caldav_username`, verifies the
   Argon2 hash) → passes through to `handle_well_known`, which always
   `301`-redirects to `/principals/`.
3. **`PROPFIND /principals/`** (with the same credentials) → `handle_principals`
   returns a `207 Multi-Status` body containing:
   - `<D:current-user-principal>` → `/principals/`
   - `<C:calendar-home-set>` → `/calendars/{username}/`, where `{username}`
     is the **authenticated** Basic Auth username (not looked up from the
     path — it's literally echoed back from `AuthenticatedCaldavUser`).
4. **`PROPFIND /calendars/{username}/`** → `handle_calendars_propfind` looks
   up all `calendars` rows whose `caldav_username` matches the authenticated
   user, and returns `<D:href>` entries pointing at each one's
   `/cal/{public_id}/`.
5. **`PROPFIND` / `GET` / `REPORT` on `/cal/{public_id}/`** → the actual
   calendar collection, resolved via `calendar_by_public_id`.

Every step after the initial `401` runs under the *same* Basic Auth
credentials — the server never needs a path segment to know who's asking,
because step 3 (`calendar-home-set`) is what tells the client "here is where
*your* calendars live," dynamically, per request.

## `public_id` vs `database_id` — why two ids

- **`public_id`** (UUID, globally unique) — used for everything
  client-facing: `/cal/{public_id}`, `/app/{public_id}`, and as the identity
  CalDAV ownership checks compare against. Introduced when `database_id`
  stopped being globally unique (one Notion database can now be subscribed
  by multiple SaaS users).
- **`database_id`** (Notion's own id) — used only for the shared in-memory
  event cache and direct Notion API calls, so two different users
  subscribed to the *same* Notion database share one cached copy of its
  events instead of double-fetching.

Legacy rows (from before this split existed) were backfilled with
`public_id = database_id`, so old bookmarked CalDAV/webview URLs keep
working unchanged.

## Legacy host aliases (pre-multi-tenant, do not extend)

`calendar.opendiy.vn` and `mytime.opendiy.vn` are hardcoded in
`get_public_id_for_host` and map directly to two specific real
`database_id`s in huyvu8051's own workspace. These predate multi-tenancy —
they exist purely so pre-existing calendar-app subscriptions to those exact
hostnames keep resolving without the discovery handshake. New tenants never
go through this path; they always resolve via Basic Auth + discovery, as
above.

## If a CalDAV client shows "cannot connect" / sync errors

Checked 2026-08-02: production logs show the discovery/auth chain working
end-to-end for real, currently-syncing clients (`401` → retry-with-auth →
`207`/`200`, zero `403`/`500` in a 6h window), and `cargo test --test
caldav_smoke_test` (which exercises this exact chain against a real HTTP
router) passes. Likely causes for a one-off failure, in order of likelihood:

1. **Transient state during a deploy** — a rolling deploy briefly runs the
   old binary against the new DB schema (or vice versa). Self-resolves once
   the rollout finishes; re-test after confirming
   `kubectl get pods -n notion-caldav-saas` shows the new pod `Running`.
2. **Client-side cache** — Apple Calendar sometimes latches onto a stale
   error state even after the server recovers. Remove and re-add the
   account rather than just waiting.
3. **Wrong host** — make sure the account was added against
   `notion-caldav.opendiy.vn` (the SaaS), not the old single-tenant
   `notion-sync.opendiy.vn` instance, which still exists separately and has
   its own single calendar.

## Known quirk (fixed 2026-08-02)

`OPTIONS` on `/cal/{public_id}/` and `/cal/{public_id}/{event_id}` used to
fall through to `405 Method Not Allowed` (the `DAV`/`Allow` headers were
still present, added by `add_caldav_headers`, but the status code itself
was wrong) because `handle_calendar_impl`/`handle_calendar_event_impl` had
no explicit `OPTIONS` branch, unlike `/principals/` and
`/calendars/{user}/` which did. Fixed to return `200 OK` like the other
discovery endpoints.
