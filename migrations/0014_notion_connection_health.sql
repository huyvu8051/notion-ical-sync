-- NULL = healthy. Set when Notion returns 401 for this connection's token
-- (revoked/expired) so the sync loop stops retrying it and we can notify
-- the owner. Cleared when they reconnect Notion for this workspace.
ALTER TABLE notion_connections
    ADD COLUMN token_invalid_since TIMESTAMPTZ,
    ADD COLUMN action_token TEXT NOT NULL DEFAULT gen_random_uuid()::text;

-- Unguessable per-connection token for the no-login "stop syncing" link
-- sent in the token-invalid notification email (same pattern as
-- users.mobileconfig_token).
CREATE UNIQUE INDEX idx_notion_connections_action_token ON notion_connections(action_token);
