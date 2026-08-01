-- One row per SaaS login (Keycloak sub claim).
CREATE TABLE users (
    id BIGSERIAL PRIMARY KEY,
    keycloak_sub TEXT NOT NULL UNIQUE,
    email TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- One Notion OAuth grant per user. A user could disconnect/reconnect (e.g.
-- switch workspace), so this is its own table rather than columns on users.
CREATE TABLE notion_connections (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    notion_access_token TEXT NOT NULL,
    workspace_id TEXT NOT NULL,
    workspace_name TEXT NOT NULL DEFAULT '',
    bot_id TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_notion_connections_workspace_id ON notion_connections(workspace_id);

-- One row per database/calendar a user chose to sync. database_id/
-- data_source_id are Notion-global unique IDs, so /cal/{db_id} paths never
-- collide across tenants.
CREATE TABLE calendars (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    notion_connection_id BIGINT NOT NULL REFERENCES notion_connections(id) ON DELETE CASCADE,
    database_id TEXT NOT NULL UNIQUE,
    data_source_id TEXT NOT NULL,
    date_property TEXT NOT NULL DEFAULT 'Date',
    display_name TEXT NOT NULL DEFAULT '',
    caldav_username TEXT NOT NULL UNIQUE,
    caldav_password_hash TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_calendars_data_source_id ON calendars(data_source_id);
CREATE INDEX idx_calendars_user_id ON calendars(user_id);
