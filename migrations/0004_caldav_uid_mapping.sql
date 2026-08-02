-- Maps a CalDAV client's own event UID (e.g. Apple Calendar's locally
-- generated "D29FFCFB-EB0C-4BE0-B6E8-BF2C5E0B0DB6") to the real Notion page
-- id that event became. Without this, a PUT for a brand-new CalDAV event
-- has no way to later recognize "this UID already exists in Notion" on a
-- follow-up PUT (edit) — the server can only match events by Notion page
-- id, and the client keeps using its own UID, so every edit looked like a
-- new event and created a fresh duplicate Notion page each time.
CREATE TABLE caldav_event_ids (
    calendar_id BIGINT NOT NULL REFERENCES calendars(id) ON DELETE CASCADE,
    caldav_uid TEXT NOT NULL,
    notion_page_id TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (calendar_id, caldav_uid)
);
