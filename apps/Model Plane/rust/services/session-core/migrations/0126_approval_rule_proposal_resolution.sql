-- 0126: an "Always" proposal carries its capability and policy, and a confirm claims the row first
-- (build plan WS13 S7, D2-1127; review findings 3 and 9).
--
-- Migration 0070 recorded a proposal and nothing read it. The row named the MCP tool and its
-- definition digest but not the capability-core id a confirmed rule is written under, and it did
-- not link to the rule a confirm wrote. This migration adds both, and adds the claim that keeps a
-- dismiss from racing a confirm.
--
--   capability_id   the registry id (`cap.mcp_tool.<name>`) the confirm writes its user-scope
--                   `allow` under, copied from the approval's cause. A proposal made before this
--                   migration keeps '' and reads as stale: it cannot be confirmed.
--   policy_id       the capability-core policy the confirm wrote, set when the row is confirmed.
--   claimed_at      when a confirm took the row. Set with state 'confirming'.
--
-- The confirm is a saga across services (model-gateway drives it, D-WS13-2): claim the row here,
-- write the rule in capability-core, record the policy here. If a dismiss could land between the
-- capability-core write and the record, a live allow would stay behind a proposal the person
-- dismissed. So a confirm first moves the row to 'confirming', and a dismiss refuses a claim made
-- in the last 60 seconds. Re-claiming one's own claim is the idempotent retry.
--
-- RESIDUAL: a confirm that dies after its capability-core write and before it records the policy
-- leaves a claim. Dismissing that claim after 60 seconds succeeds and leaves the allow in place. It
-- is revocable with DELETE /v1/tool-policies/{capability_id}?scope=user, and it lapses after 90
-- days (capability-core migration 0030).
--
-- The open index of 0070 (WHERE state = 'proposed') is unchanged: a later "Always" for the same
-- definition while a confirm is in flight creates a second proposal, which is harmless because its
-- confirm updates the same capability-core row in place.
--
-- Zero Data Retention: the new columns hold ids and timestamps, never content. A proposal exists
-- only beside a persistent run's thread grant. Rows are erased with their approval (ON DELETE
-- CASCADE), their org (gdpr.rs) and their person.
--
-- Migration 0070 is not edited. Every statement is safe to run twice. The reversal is in
-- revert/0126_approval_rule_proposal_resolution.down.sql.

ALTER TABLE approval_rule_proposals
    ADD COLUMN IF NOT EXISTS capability_id TEXT NOT NULL DEFAULT '';

ALTER TABLE approval_rule_proposals
    ADD COLUMN IF NOT EXISTS policy_id TEXT NOT NULL DEFAULT '';

ALTER TABLE approval_rule_proposals
    ADD COLUMN IF NOT EXISTS claimed_at TIMESTAMPTZ;

-- The state vocabulary gains 'confirming'. The 0070 CHECK was written inline, so it carries a
-- generated name: find it by its definition and drop it, then add the wider one when missing.
DO $$
DECLARE
    old_check TEXT;
BEGIN
    FOR old_check IN
        SELECT conname FROM pg_constraint
        WHERE conrelid = 'approval_rule_proposals'::regclass
          AND contype = 'c'
          AND pg_get_constraintdef(oid) LIKE '%proposed%'
          AND pg_get_constraintdef(oid) LIKE '%confirmed%'
          AND pg_get_constraintdef(oid) LIKE '%dismissed%'
          AND pg_get_constraintdef(oid) NOT LIKE '%confirming%'
          AND conname <> 'approval_rule_proposals_resolve_shape'
    LOOP
        EXECUTE format('ALTER TABLE approval_rule_proposals DROP CONSTRAINT %I', old_check);
    END LOOP;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conrelid = 'approval_rule_proposals'::regclass
          AND conname = 'approval_rule_proposals_state_vocab'
    ) THEN
        ALTER TABLE approval_rule_proposals
            ADD CONSTRAINT approval_rule_proposals_state_vocab
            CHECK (state IN ('proposed', 'confirming', 'confirmed', 'dismissed'));
    END IF;
END$$;

-- Only resolution sets the time, and a claim is not a resolution: an open row (proposed or
-- confirming) has no resolved_at, a resolved one has.
ALTER TABLE approval_rule_proposals
    DROP CONSTRAINT IF EXISTS approval_rule_proposals_resolve_shape;
ALTER TABLE approval_rule_proposals
    DROP CONSTRAINT IF EXISTS approval_rule_proposals_resolve_shape_v2;
ALTER TABLE approval_rule_proposals
    ADD CONSTRAINT approval_rule_proposals_resolve_shape_v2
    CHECK ((state IN ('proposed', 'confirming')) = (resolved_at IS NULL));

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conrelid = 'approval_rule_proposals'::regclass
          AND conname = 'approval_rule_proposals_claim_shape'
    ) THEN
        ALTER TABLE approval_rule_proposals
            ADD CONSTRAINT approval_rule_proposals_claim_shape
            CHECK (state <> 'confirming' OR claimed_at IS NOT NULL);
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conrelid = 'approval_rule_proposals'::regclass
          AND conname = 'approval_rule_proposals_capability_shape'
    ) THEN
        ALTER TABLE approval_rule_proposals
            ADD CONSTRAINT approval_rule_proposals_capability_shape
            CHECK (capability_id = '' OR capability_id ~ '^cap\.mcp_tool\.[A-Za-z0-9_.:-]{1,187}$');
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conrelid = 'approval_rule_proposals'::regclass
          AND conname = 'approval_rule_proposals_policy_shape'
    ) THEN
        ALTER TABLE approval_rule_proposals
            ADD CONSTRAINT approval_rule_proposals_policy_shape
            CHECK (length(policy_id) <= 128);
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conrelid = 'approval_rule_proposals'::regclass
          AND conname = 'approval_rule_proposals_confirm_shape'
    ) THEN
        -- NOT VALID: existing confirmed rows have policy_id '' (new column) and would fail the check.
        ALTER TABLE approval_rule_proposals
            ADD CONSTRAINT approval_rule_proposals_confirm_shape
            CHECK (state <> 'confirmed' OR policy_id <> '') NOT VALID;
    END IF;
END$$;
