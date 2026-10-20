-- Reverts 0003_browser_grants_budget.up.sql. Dropping the columns drops their
-- inline CHECKs and the table-level browser_grants_budget_claim_check, which
-- involves them. Idempotent.
ALTER TABLE IF EXISTS browser_grants
    DROP COLUMN IF EXISTS budget_micro_usd,
    DROP COLUMN IF EXISTS budget_max_steps,
    DROP COLUMN IF EXISTS budget_max_runtime_s,
    DROP COLUMN IF EXISTS budget_source,
    DROP COLUMN IF EXISTS budget_minted_at_unix;
