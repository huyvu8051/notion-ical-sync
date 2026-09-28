-- Switching billing provider from Stripe to Paddle; rename the generic
-- billing-provider columns so they aren't misleadingly named after the old
-- provider. RENAME COLUMN preserves existing data (no rows affected).
ALTER TABLE users RENAME COLUMN stripe_customer_id TO billing_customer_id;
ALTER TABLE users RENAME COLUMN stripe_subscription_id TO billing_subscription_id;

ALTER INDEX idx_users_stripe_customer_id RENAME TO idx_users_billing_customer_id;
