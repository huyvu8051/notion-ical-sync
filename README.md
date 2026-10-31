# Notion iCal Sync

Sync Notion databases to CalDAV / iCal clients.

## Features

- **Syncs database events** from Notion to standard `.ics` / CalDAV endpoints.
- **Read-Only by default**: CalDAV writes (e.g., PUT, DELETE, PROPPATCH from Apple Calendar or other clients) are rejected with a `403 Forbidden` response by default to protect your databases.
- **Realtime via Notion webhooks**: `/webhook/notion-test` verifies `X-Notion-Signature` (HMAC-SHA256 using the subscription's `verification_token`) and immediately refreshes the affected database on a verified event, instead of waiting for the poll loop. The poll loop itself now only runs every 10 minutes, as a fallback for anything a webhook missed.
- **Webview** at `/app/{db_id}`: a Leptos SSR+hydrate page shell around a FullCalendar view — view, create, drag-move, and delete events. Edits write straight through to the real Notion page via the API (create/PATCH/trash), then trigger an immediate refresh so the change shows up right away. Behind the same Basic Auth as the rest of the app.

## Running it locally

```sh
./scripts/dev.sh
```

Then http://127.0.0.1:8080.

That is the only way, and it is deliberately the only way. `cargo leptos
watch` is what actually serves the app — it builds the wasm hydrate bundle as
well as the server binary, which a bare `cargo run --bin notion-ical-sync`
does not, so a page served that way renders and then never comes alive. The
script exists to hand `cargo leptos watch` a database and nothing else.

Postgres lives in `.devdb/` at the repo root (gitignored) rather than a
Homebrew service: nothing registers to start at login, nothing collides with a
system Postgres on 5432, and `rm -rf .devdb` is a full reset. Migrations run
themselves on startup, so there is no separate migrate step.

Everything optional is unset, and the app says so on boot — one warning per
integration, naming the variables it wants. Notion OAuth, Paddle, email and
tracing all degrade to a not-configured page or a no-op rather than failing to
start, so the parts you are not working on stay out of the way.

`assets/style-auth-a.css` is committed, so Tailwind does not need rebuilding
unless you write a class name that is not already used somewhere. See
`tailwind/README.md` for that.

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

`CALDAV_PASSWORD_ENC_KEY` is still in `.env` and in the production sealed
secret, and nothing reads it. It went dead when CalDAV passwords moved to
show-once plus an Argon2 hash — there is no longer a stored password to
decrypt. Safe to drop from both.

## Health Check

The `/health` endpoint returns a JSON response specifying the system status and the CalDAV writes permission flag:

```json
{
  "status": "ok",
  "caldav_allow_writes": "false"
}
```
