-- 033_space_effect_policy_events: append-only history of every change to an
-- organisation's Space effect-policy entitlements (WS03-S1). One row per
-- effective change, written in the same transaction as the policy UPDATE and
-- the user_audit_outbox row. Booleans and key names only: no content, so it is
-- safe for Zero Data Retention orgs.
--
-- Adds no default row and no column on space_effect_policies: a missing
-- policy row stays a denial.

BEGIN;

CREATE TABLE IF NOT EXISTS space_effect_policy_events (
    event_id                  BIGSERIAL PRIMARY KEY,
    org_id                    TEXT NOT NULL,
    idempotency_key           TEXT,
    request_digest            TEXT,
    source                    TEXT NOT NULL,
    actor_subject_type        TEXT NOT NULL,
    actor_subject_id          TEXT NOT NULL,
    actor_org_role            TEXT,
    change_kind               TEXT NOT NULL,
    preset_id                 TEXT,
    before_entitlements       JSONB NOT NULL,
    after_entitlements        JSONB NOT NULL,
    privacy_changed           BOOLEAN NOT NULL DEFAULT FALSE,
    policy_revision_after     BIGINT NOT NULL,
    entitlement_revision_after BIGINT NOT NULL,
    spaces_fenced             INTEGER NOT NULL DEFAULT 0,
    occurred_at               TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT space_effect_policy_events_source_chk CHECK (source IN ('org_admin', 'operator')),
    CONSTRAINT space_effect_policy_events_actor_type_chk CHECK (actor_subject_type IN ('user', 'service')),
    CONSTRAINT space_effect_policy_events_actor_chk CHECK (char_length(btrim(actor_subject_id)) > 0),
    CONSTRAINT space_effect_policy_events_kind_chk CHECK (change_kind IN (
        'entitlements_set', 'preset_applied', 'operator_created', 'operator_updated'
    )),
    CONSTRAINT space_effect_policy_events_idempotency_chk CHECK (
        idempotency_key IS NULL OR char_length(idempotency_key) BETWEEN 8 AND 128
    )
);

CREATE UNIQUE INDEX IF NOT EXISTS space_effect_policy_events_idem_idx
    ON space_effect_policy_events (org_id, idempotency_key)
    WHERE idempotency_key IS NOT NULL;

CREATE INDEX IF NOT EXISTS space_effect_policy_events_org_idx
    ON space_effect_policy_events (org_id, event_id DESC);

CREATE OR REPLACE FUNCTION space_effect_policy_events_immutable() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'space_effect_policy_events is append-only (% rejected)', TG_OP
        USING ERRCODE = 'restrict_violation';
END;
$$;

DROP TRIGGER IF EXISTS space_effect_policy_events_immutable_trg ON space_effect_policy_events;
CREATE TRIGGER space_effect_policy_events_immutable_trg
    BEFORE UPDATE OR DELETE ON space_effect_policy_events
    FOR EACH ROW EXECUTE FUNCTION space_effect_policy_events_immutable();

COMMIT;
