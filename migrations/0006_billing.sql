ALTER TABLE users
    ADD COLUMN trial_started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    ADD COLUMN stripe_customer_id TEXT,
    ADD COLUMN stripe_subscription_id TEXT,
    ADD COLUMN subscription_status TEXT NOT NULL DEFAULT 'none';

-- Existing users must not get a fresh 6-month clock starting on deploy day —
-- backfill from their actual signup date.
UPDATE users SET trial_started_at = created_at;

-- customer.subscription.* webhooks only carry a Stripe customer id, not our
-- internal user id — needs a fast lookup back to the owning user.
CREATE UNIQUE INDEX idx_users_stripe_customer_id ON users(stripe_customer_id)
    WHERE stripe_customer_id IS NOT NULL;
