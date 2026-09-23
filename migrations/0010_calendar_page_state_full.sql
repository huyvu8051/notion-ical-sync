-- calendar_page_state started as a diff-only watermark table (last_edited
-- per page) so sync_log diffing had one shared, durable source across pods
-- instead of comparing against each pod's private in-memory cache. The
-- actual event content (title, start/end, location, ...) was never
-- persisted anywhere — it lived only in that same per-pod in-memory cache
-- (AppState.cache), which every CalDAV response and the webview read
-- straight from. With multiple replicas, only whichever pod last refreshed
-- from Notion/webhook had fresh data in its own memory, so requests served
-- by the *other* pod would show stale or missing events until its own next
-- periodic refresh (up to 10 minutes later).
--
-- Extending this table (instead of adding a new one) to also hold the full
-- event content makes it double as the durable, shared read path: every
-- pod now serves calendar data straight from Postgres instead of its own
-- private memory.
ALTER TABLE calendar_page_state
    ADD COLUMN title TEXT NOT NULL DEFAULT '',
    ADD COLUMN start_at TEXT NOT NULL DEFAULT '',
    ADD COLUMN end_at TEXT,
    ADD COLUMN url TEXT NOT NULL DEFAULT '',
    ADD COLUMN location TEXT,
    ADD COLUMN notes TEXT,
    ADD COLUMN priority SMALLINT,
    ADD COLUMN busy BOOLEAN,
    ADD COLUMN reminder_minutes BIGINT,
    ADD COLUMN travel_minutes BIGINT,
    ADD COLUMN repeat_rule TEXT,
    ADD COLUMN attendees TEXT[] NOT NULL DEFAULT '{}';

-- Existing rows only ever had (calendar_id, notion_page_id, last_edited)
-- populated, so the new columns above default to empty for them. The
-- upsert this table's writer uses skips the UPDATE branch whenever
-- last_edited is unchanged from Notion's point of view — the right
-- behavior for steady-state diffing, but it means those pre-existing rows
-- would never get backfilled with real content until Notion happens to
-- touch that page again. Wipe the table instead: it's a derived cache of
-- Notion, never a source of truth, and refresh_all() runs synchronously at
-- startup before the server accepts any requests, so it's repopulated
-- before anyone can observe it empty.
TRUNCATE calendar_page_state;
