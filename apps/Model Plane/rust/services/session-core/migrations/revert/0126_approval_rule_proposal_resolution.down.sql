-- Reverses 0126_approval_rule_proposal_resolution.sql: drops the proposal's capability, policy and
-- claim, and restores the 0070 state vocabulary and resolve shape.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads only the
-- top level of migrations/. Run it by hand, against the database only.
--
-- ORDER, both steps:
--   1. turn the proposal RPCs off on session-core and model-gateway, or roll both back to builds
--      that neither claim nor resolve a proposal, so no confirm is in flight while the columns go;
--   2. run this file.
--
-- A claimed proposal (state 'confirming') returns to 'proposed' first, since the 0070 vocabulary has
-- no such state. A rule a confirm already wrote in capability-core is not touched by this file and
-- stays revocable with DELETE /v1/tool-policies/{capability_id}?scope=user. A confirmed row loses
-- the link to its policy with the column.
--
-- Safe to run twice (every statement is IF EXISTS or guarded).

UPDATE approval_rule_proposals
   SET state = 'proposed', claimed_at = NULL
 WHERE state = 'confirming';

ALTER TABLE approval_rule_proposals DROP CONSTRAINT IF EXISTS approval_rule_proposals_confirm_shape;
ALTER TABLE approval_rule_proposals DROP CONSTRAINT IF EXISTS approval_rule_proposals_policy_shape;
ALTER TABLE approval_rule_proposals DROP CONSTRAINT IF EXISTS approval_rule_proposals_capability_shape;
ALTER TABLE approval_rule_proposals DROP CONSTRAINT IF EXISTS approval_rule_proposals_claim_shape;
ALTER TABLE approval_rule_proposals DROP CONSTRAINT IF EXISTS approval_rule_proposals_resolve_shape_v2;
ALTER TABLE approval_rule_proposals DROP CONSTRAINT IF EXISTS approval_rule_proposals_state_vocab;

ALTER TABLE approval_rule_proposals DROP COLUMN IF EXISTS claimed_at;
ALTER TABLE approval_rule_proposals DROP COLUMN IF EXISTS policy_id;
ALTER TABLE approval_rule_proposals DROP COLUMN IF EXISTS capability_id;

-- The 0070 definitions, restored only when missing.
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conrelid = 'approval_rule_proposals'::regclass
          AND contype = 'c'
          AND pg_get_constraintdef(oid) LIKE '%proposed%'
          AND pg_get_constraintdef(oid) LIKE '%confirmed%'
          AND pg_get_constraintdef(oid) LIKE '%dismissed%'
          AND conname <> 'approval_rule_proposals_resolve_shape'
    ) THEN
        ALTER TABLE approval_rule_proposals
            ADD CONSTRAINT approval_rule_proposals_state_check
            CHECK (state IN ('proposed', 'confirmed', 'dismissed'));
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conrelid = 'approval_rule_proposals'::regclass
          AND conname = 'approval_rule_proposals_resolve_shape'
    ) THEN
        ALTER TABLE approval_rule_proposals
            ADD CONSTRAINT approval_rule_proposals_resolve_shape
            CHECK ((state = 'proposed') = (resolved_at IS NULL));
    END IF;
END$$;
