# Notion iCal Sync

Sync Notion databases to CalDAV / iCal clients.

## Features

- **Syncs database events** from Notion to standard `.ics` / CalDAV endpoints.
- **Read-Only by default**: CalDAV writes (e.g., PUT, DELETE, PROPPATCH from Apple Calendar or other clients) are rejected with a `403 Forbidden` response by default to protect your databases.
- **Realtime via Notion webhooks**: `/webhook/notion-test` verifies `X-Notion-Signature` (HMAC-SHA256 using the subscription's `verification_token`) and immediately refreshes the affected database on a verified event, instead of waiting for the poll loop. The poll loop itself now only runs every 10 minutes, as a fallback for anything a webhook missed.
- **Webview** at `/app/{db_id}`: a Leptos SSR+hydrate page shell around a FullCalendar view — view, create, drag-move, and delete events. Edits write straight through to the real Notion page via the API (create/PATCH/trash), then trigger an immediate refresh so the change shows up right away. Behind the same Basic Auth as the rest of the app.

## Running it locally

```sh
cargo leptos watch
```

Then http://127.0.0.1:8080.

That is the whole of it, and it has to stay the whole of it. `cargo leptos
watch` builds the wasm hydrate bundle as well as the server binary; a bare
`cargo run --bin notion-ical-sync` does not, so pages served that way render
and then never come alive — they look right and are subtly dead.

It needs no arguments because the defaults already fit: `main.rs` loads `.env`
through dotenvy, and `DATABASE_URL` falls back to
`postgres://notion-caldav-saas:notion-caldav-saas@localhost:5433/notion-caldav-saas`
when unset. Migrations run themselves on startup.

That fallback used to name a different project -- `biolink:biolink` on a
`notion_saas` database, neither of which belongs to this app. If your local
Postgres on :5433 still only has the old role, either put a `DATABASE_URL` in
`.env` or create the new one once:

```sh
psql -h localhost -p 5433 -U postgres -c 'CREATE ROLE "notion-caldav-saas" LOGIN PASSWORD '"'"'notion-caldav-saas'"'"';'
psql -h localhost -p 5433 -U postgres -c 'CREATE DATABASE "notion-caldav-saas" OWNER "notion-caldav-saas";'
```

The quotes are not optional: the hyphens make it a quoted identifier
everywhere. The name matches the Kubernetes namespace and the role in the
shared Postgres cluster, which is the point -- one name for this app in every
environment.

Everything optional is unset, and the app says so on boot — one warning per
integration, naming the variables it wants. Notion OAuth, Paddle, email and
tracing each degrade to a not-configured page or a no-op rather than failing
to start, so the parts you are not working on stay out of the way.

To leave your working data alone — seeding rows to exercise a code path,
testing a migration, wanting a first-run account —
`./scripts/leptos-watch-with-db.sh` runs the same command against a throwaway
Postgres in `.devdb/`. That is the only reason to use it; for everything else
the bare command above is the way.

`assets/` is gitignored and the stylesheet in it is generated, not tracked.
The Docker build regenerates it from scratch, so production always has every
utility class the code uses — while the local copy is whatever you last built,
possibly months ago. A class that is new to the codebase therefore works in
production and does nothing on your machine until you rebuild:

```sh
cd tailwind && npx tailwindcss@3.4.17 -c auth-a.config.js -i input.css -o ../assets/style-auth-a.css --minify
```

Worth knowing before you conclude a style change had no effect.

## Environment Variables

`DATABASE_URL` is the only one the app cannot start without, and
`scripts/dev.sh` sets it. Everything else is optional: an unset variable
disables its own feature and says so in one startup warning, rather than
stopping the boot.

| Variable | Unset means |
|----------|-------------|
| `PORT` | listens on `8080` |
| `APP_BASE_URL` | links in emails and OAuth redirects fall back to a default |
| `KEYCLOAK_ISSUER_URL` / `KEYCLOAK_CLIENT_ID` / `KEYCLOAK_CLIENT_SECRET` | no login — the public pages still render, which is enough for working on them |
| `NOTION_OAUTH_CLIENT_ID` / `NOTION_OAUTH_CLIENT_SECRET` | the Connect Notion flow shows a not-configured page |
| `NOTION_WEBHOOK_SECRET` | webhook events are logged and dropped, since the signature cannot be verified |
| `NOTION_API_BASE_URL` | talks to the real Notion API |
| `MOCK_NOTION_PORT` | no mock Notion server |
| `PADDLE_API_KEY` / `PADDLE_WEBHOOK_SECRET` / `PADDLE_PRICE_ID` / `PADDLE_CLIENT_TOKEN` | `/billing/checkout` shows a not-configured page. Sandbox or live is read from the API key's own prefix, never from a second variable, so the two cannot drift apart |
| `RESEND_API_KEY` / `EMAIL_FROM` | no transactional email is sent |
| `ADMIN_SECRET` | `/admin/reset-billing` is disabled |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | traces and logs go to stdout only |
| `MAPBOX_ACCESS_TOKEN` | the event location field is plain text with no autocomplete |
| `CALDAV_ALLOW_WRITES` | CalDAV writes are rejected with 403 |

## Health Check

The `/health` endpoint returns a JSON response specifying the system status and the CalDAV writes permission flag:

```json
{
  "status": "ok",
  "caldav_allow_writes": "false"
}
```
