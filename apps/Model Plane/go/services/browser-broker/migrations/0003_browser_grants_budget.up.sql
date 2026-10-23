-- browser-broker 0003: the grant-carried budget claim (WS6.E, R-3 allocation:
-- 0002 stays reserved for WS7, gaps are tolerated).
--
-- The container entrypoint re-applies every *.up.sql on each start, so every
-- statement here is idempotent: ADD COLUMN IF NOT EXISTS skips the whole
-- clause (its inline CHECK included) when the column exists, and the one
-- table-level constraint is added only when pg_constraint does not have it.
--
-- All columns are nullable and there is no backfill: an old row, and a grant
-- minted without a claim, read back as "no claim" (NULL is stored for a zero
-- bound; the Go layer reads COALESCE(col, 0)). A claim is three bounds and a
-- label, never content, and Zero Data Retention grants never reach this table
-- (X-2), so there is nothing here a ZDR posture could forbid.
--
-- budget_minted_at_unix is the fifth proto field (44). The plan's migration
-- named four columns; the broker stamps this one itself, and it needs a place
-- to live so a replay or a later validate returns the same value.

ALTER TABLE browser_grants
    ADD COLUMN IF NOT EXISTS budget_micro_usd BIGINT
        CHECK (budget_micro_usd IS NULL OR budget_micro_usd > 0),
    ADD COLUMN IF NOT EXISTS budget_max_steps INT
        CHECK (budget_max_steps IS NULL OR budget_max_steps > 0),
    ADD COLUMN IF NOT EXISTS budget_max_runtime_s INT
        CHECK (budget_max_runtime_s IS NULL OR budget_max_runtime_s > 0),
    ADD COLUMN IF NOT EXISTS budget_source TEXT
        CHECK (budget_source IS NULL OR budget_source IN ('request', 'org_default', 'entitlement')),
    ADD COLUMN IF NOT EXISTS budget_minted_at_unix BIGINT
        CHECK (budget_minted_at_unix IS NULL OR budget_minted_at_unix > 0);

-- A source labels a bound and a bound needs a label: both or neither.
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'browser_grants_budget_claim_check'
          AND conrelid = 'browser_grants'::regclass
    ) THEN
        ALTER TABLE browser_grants ADD CONSTRAINT browser_grants_budget_claim_check
            CHECK (
                (budget_source IS NULL) =
                (budget_micro_usd IS NULL AND budget_max_steps IS NULL AND budget_max_runtime_s IS NULL)
            );
    END IF;
END
$$;
