-- Verevon 14-day Pro trial: billing-core owns the trial window.
-- On organization.created the account is provisioned as `trialing` with a
-- trial_ends_at = now + TRIAL_DURATION_DAYS; while trialing the account's
-- effective plan + entitlements are elevated to Pro. A periodic sweep reverts
-- expired trials to the base (free) plan.

ALTER TABLE billing_accounts
    ADD COLUMN IF NOT EXISTS trial_ends_at TIMESTAMPTZ NULL;

-- Sweep query: find still-trialing accounts whose window has elapsed.
CREATE INDEX IF NOT EXISTS idx_billing_accounts_trialing
    ON billing_accounts (trial_ends_at)
    WHERE subscription_state = 'trialing';
