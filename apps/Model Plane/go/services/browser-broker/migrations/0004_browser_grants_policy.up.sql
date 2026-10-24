-- browser-broker 0004: human_browse and attach grants (WS8.C; R-3 allocation:
-- 0002 stays reserved for WS7, 0003 is the budget claim). The plan's design
-- named this file 0002_browser_grants_policy; R-3 moved it.
--
-- The container entrypoint re-applies every *.up.sql on each start, so every
-- statement here is idempotent: ADD COLUMN IF NOT EXISTS, CREATE INDEX IF NOT
-- EXISTS, and each constraint is replaced only when the stored definition does
-- not already mention the new kinds.
--
-- What changes:
--   * policy_mode      'allowlist' (every grant before WS8.C) or 'open_web'
--                      (a human_browse grant: all public hosts but denied_domains).
--   * denied_domains   an open-web grant's deny-list; empty otherwise.
--   * resource_id      the run an attach grant is bound to; NULL otherwise.
--   * kind             gains 'human_browse' and 'attach'.
--   * allowed_domains  may be empty for a human_browse grant only (open web is
--                      its policy mode, never an empty allow-list elsewhere).
--   * parent           a human_browse grant is a root like a session grant.
--   * absolute_expires_at is already in 0001; a human_browse grant sets it to
--                      its creation time plus BROWSER_BROKER_HUMAN_ABSOLUTE_TTL_SECONDS
--                      and a renewal can never pass it.
--
-- Still no zdr column: a ZDR human_browse or attach grant lives in the broker's
-- memory with a "z_" id and never reaches this table (X-2).

ALTER TABLE browser_grants
    ADD COLUMN IF NOT EXISTS policy_mode TEXT NOT NULL DEFAULT 'allowlist',
    ADD COLUMN IF NOT EXISTS denied_domains TEXT[] NOT NULL DEFAULT '{}',
    ADD COLUMN IF NOT EXISTS resource_id TEXT;

DO $$
BEGIN
    -- kind: add 'human_browse' and 'attach'.
    IF EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'browser_grants_kind_check'
          AND conrelid = 'browser_grants'::regclass
          AND pg_get_constraintdef(oid) NOT LIKE '%human_browse%'
    ) THEN
        ALTER TABLE browser_grants DROP CONSTRAINT browser_grants_kind_check;
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'browser_grants_kind_check'
          AND conrelid = 'browser_grants'::regclass
    ) THEN
        ALTER TABLE browser_grants ADD CONSTRAINT browser_grants_kind_check
            CHECK (kind IN ('session', 'human_browse', 'run', 'attach', 'sensitive_approval'));
    END IF;

    -- allowed_domains: 1 to 32 hosts, except an open-web grant holds none.
    IF EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'browser_grants_domains_check'
          AND conrelid = 'browser_grants'::regclass
          AND pg_get_constraintdef(oid) NOT LIKE '%human_browse%'
    ) THEN
        ALTER TABLE browser_grants DROP CONSTRAINT browser_grants_domains_check;
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'browser_grants_domains_check'
          AND conrelid = 'browser_grants'::regclass
    ) THEN
        ALTER TABLE browser_grants ADD CONSTRAINT browser_grants_domains_check
            CHECK (
                cardinality(allowed_domains) <= 32
                AND (kind = 'human_browse' OR cardinality(allowed_domains) >= 1)
            );
    END IF;

    -- parent: session and human_browse grants are roots; every other kind has one.
    IF EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'browser_grants_parent_check'
          AND conrelid = 'browser_grants'::regclass
          AND pg_get_constraintdef(oid) NOT LIKE '%human_browse%'
    ) THEN
        ALTER TABLE browser_grants DROP CONSTRAINT browser_grants_parent_check;
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'browser_grants_parent_check'
          AND conrelid = 'browser_grants'::regclass
    ) THEN
        ALTER TABLE browser_grants ADD CONSTRAINT browser_grants_parent_check
            CHECK ((kind IN ('session', 'human_browse')) = (parent_grant_id IS NULL));
    END IF;

    -- policy mode: open web is exactly the human_browse kind, and it carries a
    -- deny-list and no allow-list; an allowlist grant carries no deny-list.
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'browser_grants_policy_mode_check'
          AND conrelid = 'browser_grants'::regclass
    ) THEN
        ALTER TABLE browser_grants ADD CONSTRAINT browser_grants_policy_mode_check
            CHECK (
                policy_mode IN ('allowlist', 'open_web')
                AND ((policy_mode = 'open_web') = (kind = 'human_browse'))
                AND (policy_mode = 'open_web' OR cardinality(denied_domains) = 0)
                AND (kind <> 'human_browse'
                     OR (cardinality(allowed_domains) = 0 AND cardinality(denied_domains) >= 1))
            );
    END IF;

    -- resource: an attach grant is bound to one run, and nothing else is.
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'browser_grants_resource_check'
          AND conrelid = 'browser_grants'::regclass
    ) THEN
        ALTER TABLE browser_grants ADD CONSTRAINT browser_grants_resource_check
            CHECK (
                (kind = 'attach') = (resource_id IS NOT NULL)
                AND (resource_id IS NULL OR (resource_id <> '' AND octet_length(resource_id) <= 256))
            );
    END IF;
END
$$;

-- Derive of an attach grant is idempotent on (parent, session_key), like a run
-- grant (browser_grants_run_idempotency_idx in 0001).
CREATE UNIQUE INDEX IF NOT EXISTS browser_grants_attach_idempotency_idx
    ON browser_grants (parent_grant_id, session_key)
    WHERE kind = 'attach';

-- The per-owner cap counts live human_browse grants beside session grants.
CREATE INDEX IF NOT EXISTS browser_grants_owner_human_active_idx
    ON browser_grants (org_id, owner_id, expires_at)
    WHERE kind = 'human_browse' AND revoked_at IS NULL;
