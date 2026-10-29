-- Control-owned authorization linearization ledger for a private owner-plane
-- effect. It stores only immutable commitments and current-authority facts;
-- the signed decision bearer, ticket content, and service credentials never
-- cross this durable boundary.
CREATE TABLE IF NOT EXISTS space_owner_effect_reservations (
    operation_id TEXT PRIMARY KEY,
    reservation_id TEXT NOT NULL UNIQUE,
    org_id TEXT NOT NULL,
    space_ref TEXT NOT NULL REFERENCES registered_spaces(space_ref) ON DELETE RESTRICT,
    subject_id TEXT NOT NULL,
    run_id TEXT NOT NULL,
    thread_id TEXT NOT NULL,
    action_id TEXT NOT NULL CHECK (action_id = 'tickets.create'),
    action_schema_hash TEXT NOT NULL CHECK (action_schema_hash ~ '^sha256:[0-9a-f]{64}$'),
    payload_digest TEXT NOT NULL CHECK (payload_digest ~ '^sha256:[0-9a-f]{64}$'),
    idempotency_key TEXT NOT NULL,
    decision_ref TEXT NOT NULL,
    grant_ref TEXT NOT NULL,
    recipient_audience_ref TEXT NOT NULL,
    recipient_audience_hash TEXT NOT NULL,
    recipient_audience_revision BIGINT NOT NULL CHECK (recipient_audience_revision > 0),
    privacy_policy_ref TEXT NOT NULL,
    authority_revision BIGINT NOT NULL CHECK (authority_revision > 0),
    status TEXT NOT NULL CHECK (status IN ('reserved', 'committed', 'cancelled')),
    expires_at TIMESTAMPTZ NOT NULL,
    committed_at TIMESTAMPTZ,
    cancelled_at TIMESTAMPTZ,
    cancellation_reason TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (
        (status = 'reserved' AND committed_at IS NULL AND cancelled_at IS NULL AND cancellation_reason = '')
        OR (status = 'committed' AND committed_at IS NOT NULL AND cancelled_at IS NULL AND cancellation_reason = '')
        OR (status = 'cancelled' AND committed_at IS NULL AND cancelled_at IS NOT NULL AND cancellation_reason <> '')
    )
);

CREATE INDEX IF NOT EXISTS space_owner_effect_reservations_uncommitted_by_space_idx
    ON space_owner_effect_reservations (space_ref, status, created_at)
    WHERE status = 'reserved';

-- Every authority-revision mutation is a revocation fence. Cancelling only
-- uncommitted rows is deliberate: commit is the authorization linearization
-- point, so a later revoke cannot rewrite its order. An owner plane must still
-- perform its independent local-grant check before making an effect visible.
CREATE OR REPLACE FUNCTION cancel_pending_owner_effect_reservations()
RETURNS TRIGGER AS $$
BEGIN
    IF NEW.authority_revision IS DISTINCT FROM OLD.authority_revision THEN
        UPDATE space_owner_effect_reservations
        SET status = 'cancelled',
            cancelled_at = NOW(),
            cancellation_reason = 'control_authority_changed',
            updated_at = NOW()
        WHERE space_ref = NEW.space_ref
          AND status = 'reserved';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS cancel_pending_owner_effect_reservations_on_authority_change
    ON space_authority_revisions;
CREATE TRIGGER cancel_pending_owner_effect_reservations_on_authority_change
AFTER UPDATE ON space_authority_revisions
FOR EACH ROW
EXECUTE FUNCTION cancel_pending_owner_effect_reservations();
