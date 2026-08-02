-- Persistent, per-calendar history of write attempts (create/update/delete),
-- from either a CalDAV client or the webview — so a user can see what
-- happened (and whether it actually reached Notion) from a browser instead
-- of asking someone to read `kubectl logs`.
CREATE TABLE sync_log (
    id BIGSERIAL PRIMARY KEY,
    calendar_id BIGINT NOT NULL REFERENCES calendars(id) ON DELETE CASCADE,
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    source TEXT NOT NULL, -- 'caldav' or 'webview'
    action TEXT NOT NULL, -- 'create', 'update', 'delete'
    event_uid TEXT NOT NULL DEFAULT '',
    notion_page_id TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL, -- 'ok' or 'error'
    detail TEXT NOT NULL DEFAULT ''
);

CREATE INDEX idx_sync_log_calendar_id_occurred_at ON sync_log(calendar_id, occurred_at DESC);
