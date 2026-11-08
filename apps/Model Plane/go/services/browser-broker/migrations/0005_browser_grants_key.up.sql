-- browser-broker 0005: API-key grants (INNH-10 MP-2; D12 allocation: 0002 stays
-- reserved for WS7, 0003 is the budget claim, 0004 human_browse and attach).
--
-- The container entrypoint re-applies every *.up.sql on each start, so every
-- statement here is idempotent: ADD COLUMN IF NOT EXISTS, CREATE INDEX IF NOT
-- EXISTS, and the constraint is added only when it is missing.
--
-- What changes:
--   * key_id   the API key a key grant was issued for (never the key itself);
--              NULL for every other grant.
--   * a key grant is an allow-list session grant with no parent and a budget
--     claim that bounds spend, steps and runtime (the broker refuses any other
--     shape before it writes; this is the table's own fence).
--   * an index for the per-key live-grant count and the revoke-by-key sweep.
--
-- Still no zdr column: a ZDR key grant lives in the broker's memory with a "z_"
-- id and never reaches this table (X-2).

ALTER TABLE browser_grants
    ADD COLUMN IF NOT EXISTS key_id TEXT;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'browser_grants_key_check'
          AND conrelid = 'browser_grants'::regclass
    ) THEN
        ALTER TABLE browser_grants ADD CONSTRAINT browser_grants_key_check
            CHECK (
                key_id IS NULL
                OR (
                    key_id <> '' AND octet_length(key_id) <= 128
                    AND kind = 'session'
                    AND policy_mode = 'allowlist'
                    AND parent_grant_id IS NULL
                    AND cardinality(allowed_domains) >= 1
                    -- COALESCE: a CHECK passes on NULL, and no claim is NULL here.
                    AND COALESCE(budget_micro_usd, 0) > 0
                    AND COALESCE(budget_max_steps, 0) > 0
                    AND COALESCE(budget_max_runtime_s, 0) > 0
                )
            );
    END IF;
END
$$;

-- Live grants of one key (the per-key cap) and the revoke-by-key sweep.
CREATE INDEX IF NOT EXISTS browser_grants_key_idx
    ON browser_grants (key_id, org_id, expires_at)
    WHERE key_id IS NOT NULL;
