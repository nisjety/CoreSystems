ALTER TABLE billing_accounts
  ADD COLUMN IF NOT EXISTS plan_revision BIGINT NOT NULL DEFAULT 0;

DO $constraint$
BEGIN
  IF NOT EXISTS (
    SELECT 1 FROM pg_constraint
    WHERE conname = 'billing_accounts_plan_revision_nonnegative'
      AND conrelid = 'billing_accounts'::regclass
  ) THEN
    ALTER TABLE billing_accounts
      ADD CONSTRAINT billing_accounts_plan_revision_nonnegative
      CHECK (plan_revision >= 0);
  END IF;
END
$constraint$;
