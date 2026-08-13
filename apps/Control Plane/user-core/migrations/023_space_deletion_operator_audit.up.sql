-- 023_space_deletion_operator_audit — retain immutable, Control-owned proof
-- of policy and legal-hold changes. These records are operator evidence, not
-- a replacement for the per-plane purge receipts.

BEGIN;

CREATE TABLE IF NOT EXISTS space_deletion_operator_events (
    event_id            BIGSERIAL PRIMARY KEY,
    org_id              TEXT NOT NULL,
    space_ref           TEXT,
    event_type          TEXT NOT NULL,
    actor_principal_id  TEXT NOT NULL,
    hold_ref            TEXT,
    deletion_entitled   BOOLEAN,
    personal_rollout_enabled BOOLEAN,
    occurred_at         TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT space_deletion_operator_events_type_chk CHECK (
        event_type IN ('policy_updated', 'legal_hold_applied', 'legal_hold_released')
    ),
    CONSTRAINT space_deletion_operator_events_actor_chk CHECK (
        char_length(btrim(actor_principal_id)) > 0
    )
);

CREATE INDEX IF NOT EXISTS idx_space_deletion_operator_events_space
    ON space_deletion_operator_events (space_ref, event_id DESC)
    WHERE space_ref IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_space_deletion_operator_events_org
    ON space_deletion_operator_events (org_id, event_id DESC);

COMMIT;
