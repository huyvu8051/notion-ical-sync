# Notion iCal Sync

Sync Notion databases to CalDAV / iCal clients.

## Features

- **Syncs database events** from Notion to standard `.ics` / CalDAV endpoints.
- **Read-Only by default**: CalDAV writes (e.g., PUT, DELETE, PROPPATCH from Apple Calendar or other clients) are rejected with a `403 Forbidden` response by default to protect your databases.
- **Realtime via Notion webhooks**: `/webhook/notion-test` verifies `X-Notion-Signature` (HMAC-SHA256 using the subscription's `verification_token`) and immediately refreshes the affected database on a verified event, instead of waiting for the poll loop. The poll loop itself now only runs every 10 minutes, as a fallback for anything a webhook missed.
- **Webview** at `/app/{db_id}`: a server-rendered (Leptos SSR, no wasm/hydration) FullCalendar page — view, create, drag-move, and delete events. Edits write straight through to the real Notion page via the API (create/PATCH/trash), then trigger an immediate refresh so the change shows up right away. Behind the same Basic Auth as the rest of the app.

## Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `PORT` | Server listening port | `8080` |
| `NOTION_TOKEN` | Notion API Token | (Required) |
| `DATABASE_IDS` | Comma-separated list of database IDs to sync | `""` |
| `DATA_SOURCE_IDS` | Comma-separated list of data source IDs | `""` |
| `DATE_PROPERTY` | Name of the Notion date property to query | `"Date"` |
| `CALDAV_ALLOW_WRITES` | Control CalDAV write permissions. Set to `true` to allow writes; `false` (default) or `inbox` rejects writes. | `false` |
| `CALDAV_USERNAME` | Username for CalDAV Basic Authentication | (Bypassed if empty) |
| `CALDAV_PASSWORD` | Password for CalDAV Basic Authentication | (Bypassed if empty) |
| `NOTION_WEBHOOK_SECRET` | The `verification_token` Notion issued for the webhook subscription, used as the HMAC key to verify `X-Notion-Signature`. Without it, webhook events are logged but dropped. | (unset = webhook disabled) |

## Health Check

The `/health` endpoint returns a JSON response specifying the system status and the CalDAV writes permission flag:

```json
{
  "status": "ok",
  "caldav_allow_writes": "false"
}
```

## Development

To set up the pre-commit Git hook that automatically formats your code before committing, run:

```bash
git config core.hooksPath .githooks
```

