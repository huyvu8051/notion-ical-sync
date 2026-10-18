-- Which Notion property backs each calendar field, chosen by the user instead
-- of guessed from hardcoded English names.
--
-- An empty object means "nothing mapped yet" — ColumnMapping::resolve falls
-- back to the old guess (English name, case-insensitive) for those fields, so
-- calendars created before this migration keep their existing behaviour until
-- the backfill fills them in.
--
-- date_property stays its own column: it is mandatory, and it is interpolated
-- into the filter/sorts of every Notion query rather than read off a page.
ALTER TABLE calendars
    ADD COLUMN column_mapping jsonb NOT NULL DEFAULT '{}'::jsonb;
