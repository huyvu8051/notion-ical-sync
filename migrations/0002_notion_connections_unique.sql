-- A user reconnecting the same Notion workspace (e.g. after revoking and
-- re-granting access) should update the existing grant, not accumulate
-- duplicate rows with stale tokens.
ALTER TABLE notion_connections
    ADD CONSTRAINT notion_connections_user_workspace_key UNIQUE (user_id, workspace_id);
