ALTER TABLE billing_accounts
  DROP CONSTRAINT IF EXISTS billing_accounts_plan_revision_nonnegative;
ALTER TABLE billing_accounts
  DROP COLUMN IF EXISTS plan_revision;
