# Notion iCal Sync

Sync Notion databases to CalDAV / iCal clients.

## Features

- **Syncs database events** from Notion to standard `.ics` / CalDAV endpoints.
- **Read-Only by default**: CalDAV writes (e.g., PUT, DELETE, PROPPATCH from Apple Calendar or other clients) are rejected with a `403 Forbidden` response by default to protect your databases.

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

## Health Check

The `/health` endpoint returns a JSON response specifying the system status and the CalDAV writes permission flag:

```json
{
  "status": "ok",
  "caldav_allow_writes": "false"
}
```
