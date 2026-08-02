-- Allow the same Notion database to be subscribed by multiple SaaS accounts.
-- Public routes (/cal/{id}, /app/{id}) and CalDAV auth ownership checks now
-- key off each row's own public_id instead of the shared database_id, so
-- two different users' calendars for the same Notion database get distinct,
-- unambiguous URLs and credentials.
ALTER TABLE calendars DROP CONSTRAINT calendars_database_id_key;

ALTER TABLE calendars ADD COLUMN public_id TEXT;
-- Backfill: reuse database_id as public_id for pre-existing rows so their
-- CalDAV URLs and webview links keep working unchanged after this migration.
UPDATE calendars SET public_id = database_id WHERE public_id IS NULL;
ALTER TABLE calendars ALTER COLUMN public_id SET NOT NULL;
ALTER TABLE calendars ADD CONSTRAINT calendars_public_id_key UNIQUE (public_id);

-- A user still can't add the exact same database to their own account twice.
ALTER TABLE calendars ADD CONSTRAINT calendars_user_database_key UNIQUE (user_id, database_id);

-- Reversible CalDAV password storage (previously Argon2-hash-only, so the
-- plaintext existed for exactly one page render at creation time and was
-- gone forever after). Lets the dashboard show the current password again
-- later, not just once. caldav_password_hash keeps being what CalDAV Basic
-- Auth actually verifies against — this column is purely for the "reveal"
-- UI action. Empty for rows created before this migration (that password
-- was never stored anywhere recoverable) — those need "Tạo lại" instead.
ALTER TABLE calendars ADD COLUMN caldav_password_encrypted TEXT NOT NULL DEFAULT '';
