-- Why a calendar stopped syncing, when the reason is on Notion's side.
--
-- A database the user moved to the Notion trash, or deleted, answers the query
-- endpoint with 404 "Could not find data_source with ID: ... Make sure the
-- relevant pages and databases are shared with your integration" — the same
-- wording Notion uses for a permission problem. Measured on production
-- 2026-10-19: 19 of 123 calendars were in this state, and the sync loop had
-- been retrying every one of them six times an hour for days with nothing
-- surfaced to the owner.
--
-- source_state is '' while the data source answers normally, 'trashed' when it
-- is in Notion's trash (the user can undo that themselves), and 'gone' when it
-- no longer resolves at all. source_checked_at throttles re-probing, so a dead
-- calendar costs one request an hour instead of one every ten minutes.
ALTER TABLE calendars
    ADD COLUMN source_state TEXT NOT NULL DEFAULT '',
    ADD COLUMN source_checked_at TIMESTAMPTZ;
