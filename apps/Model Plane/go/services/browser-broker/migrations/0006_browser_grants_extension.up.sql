-- browser-broker 0006: origin-extension grants (E3.2-e1, OD13; D12 allocation:
-- 0002 stays reserved for WS7, 0003 is the budget claim, 0004 human_browse and
-- attach, 0005 API-key grants).
--
-- The container entrypoint re-applies every *.up.sql on each start, so every
-- statement here is idempotent: ADD COLUMN IF NOT EXISTS, CREATE INDEX IF NOT
-- EXISTS, and each constraint is replaced only when the stored definition does
-- not already mention the new kind.
--
-- What changes:
--   * consent_scope   'turn', 'conversation' or 'always' for an origin_extension
--                     grant; NULL for every other kind.
--   * kind            gains 'origin_extension': a child of one session or run
--                     grant that adds the hosts a person consented to.
--   * an extension is an allow-list grant with a parent, one to eight exact
--     hosts, no sensitive scope and no key; the broker refuses any other shape
--     before it writes, this is the table's own fence.
--   * an index for the per-parent live-extension count.
--
-- Still no zdr column: a ZDR extension lives in the broker's memory with a "z_"
-- id and never reaches this table (X-2).

ALTER TABLE browser_grants
    ADD COLUMN IF NOT EXISTS consent_scope TEXT;

DO $$
BEGIN
    -- kind: add 'origin_extension'.
    IF EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'browser_grants_kind_check'
          AND conrelid = 'browser_grants'::regclass
          AND pg_get_constraintdef(oid) NOT LIKE '%origin_extension%'
    ) THEN
        ALTER TABLE browser_grants DROP CONSTRAINT browser_grants_kind_check;
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'browser_grants_kind_check'
          AND conrelid = 'browser_grants'::regclass
    ) THEN
        ALTER TABLE browser_grants ADD CONSTRAINT browser_grants_kind_check
            CHECK (kind IN ('session', 'human_browse', 'run', 'attach', 'sensitive_approval', 'origin_extension'));
    END IF;

    -- The consent scope exists exactly on an extension, and an extension has a
    -- parent (browser_grants_parent_check already demands one for every kind
    -- that is not session or human_browse) and the shape described above.
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'browser_grants_extension_check'
          AND conrelid = 'browser_grants'::regclass
    ) THEN
        ALTER TABLE browser_grants ADD CONSTRAINT browser_grants_extension_check
            CHECK (
                (kind = 'origin_extension') = (consent_scope IS NOT NULL)
                AND (consent_scope IS NULL OR consent_scope IN ('turn', 'conversation', 'always'))
                AND (
                    kind <> 'origin_extension'
                    OR (
                        parent_grant_id IS NOT NULL
                        AND policy_mode = 'allowlist'
                        AND key_id IS NULL
                        AND cardinality(allowed_domains) BETWEEN 1 AND 8
                        AND cardinality(allowed_actions) = 0
                    )
                )
            );
    END IF;
END
$$;

-- Live extensions of one parent (the per-parent host and count cap).
CREATE INDEX IF NOT EXISTS browser_grants_extension_parent_idx
    ON browser_grants (parent_grant_id, expires_at)
    WHERE kind = 'origin_extension' AND revoked_at IS NULL;
