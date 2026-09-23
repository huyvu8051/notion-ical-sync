ALTER TABLE users
    ADD COLUMN account_caldav_username TEXT UNIQUE,
    ADD COLUMN account_caldav_password_hash TEXT;
