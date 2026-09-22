-- Durable, cross-pod record of the last known last_edited_time seen for
-- each Notion page, per calendar. sync_log diffing used to compare against
-- the in-memory page cache, which is per-pod and resets on every restart —
-- with multiple replicas each catching up independently, the same change
-- got logged once per pod instead of once. This table is the single shared
-- source of truth diffing checks against, updated with an atomic UPSERT so
-- concurrent pods can't both log the same change.
CREATE TABLE calendar_page_state (
    calendar_id BIGINT NOT NULL REFERENCES calendars(id) ON DELETE CASCADE,
    notion_page_id TEXT NOT NULL,
    last_edited TEXT NOT NULL,
    PRIMARY KEY (calendar_id, notion_page_id)
);
