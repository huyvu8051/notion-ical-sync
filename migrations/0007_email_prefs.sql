ALTER TABLE users
    ADD COLUMN preferred_lang TEXT NOT NULL DEFAULT 'vi',
    ADD COLUMN trial_reminder_sent_at TIMESTAMPTZ;
