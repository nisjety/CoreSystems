-- 042_space_message_lifecycle: Space message revision authority (WS12-S5).
--
-- Two deny-by-default org entitlements gate the Control decision
-- `model.thread.revise`: message_revise_entitled (edit or delete one's own
-- message, retract an agent reply one started) and message_moderate_entitled
-- (a room manager or owner deleting another member's message). The org's
-- original-text rule is data, not code: space_message_policies holds one row
-- per organization that chose a rule, and ABSENCE MEANS 'purge' (the safe
-- default). Control reads it into every revise decision as exactly one of
-- revise:purge_original / revise:retain_original; Session Core obeys the claim.
-- space_message_policy_events is operator evidence, one row per write attempt.
--
-- Idempotent: every statement tolerates a re-run.

BEGIN;

ALTER TABLE space_effect_policies
    ADD COLUMN IF NOT EXISTS message_revise_entitled BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN IF NOT EXISTS message_moderate_entitled BOOLEAN NOT NULL DEFAULT FALSE;

CREATE TABLE IF NOT EXISTS space_message_policies (
    org_id TEXT PRIMARY KEY,
    original_retention TEXT NOT NULL CHECK (original_retention IN ('retain', 'purge')),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS space_message_policy_events (
    event_id BIGSERIAL PRIMARY KEY,
    org_id TEXT NOT NULL,
    actor_principal_id TEXT NOT NULL,
    original_retention TEXT NOT NULL CHECK (original_retention IN ('retain', 'purge')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_space_message_policy_events_org
    ON space_message_policy_events (org_id, event_id DESC);

COMMIT;
