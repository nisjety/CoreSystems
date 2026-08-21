-- 030: exact owner-resource grants for private Model ticket effects.
--
-- A signed Control decision proves the source run and current Space policy.
-- It never grants access to an Application conversation. This table is the
-- separate owner-plane intersection: one active row authorizes one subject and
-- Space/audience/privacy binding for one conversation/action. It stores only
-- identifiers and policy references, never a bearer or conversation content.

CREATE TABLE IF NOT EXISTS conversation_agent_action_grants (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL,
    conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    action_id TEXT NOT NULL CHECK (action_id = 'tickets.create'),
    space_ref TEXT NOT NULL,
    subject_id TEXT NOT NULL,
    recipient_audience_ref TEXT NOT NULL,
    recipient_audience_hash TEXT NOT NULL,
    recipient_audience_revision BIGINT NOT NULL CHECK (recipient_audience_revision > 0),
    privacy_policy_ref TEXT NOT NULL,
    authority_revision BIGINT NOT NULL CHECK (authority_revision > 0),
    created_by_user_id TEXT NOT NULL,
    created_idempotency_key TEXT NOT NULL,
    create_request_sha256 TEXT NOT NULL,
    created_audit_event_id TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    revoked_at TIMESTAMPTZ,
    revoked_by_user_id TEXT NOT NULL DEFAULT '',
    revoked_idempotency_key TEXT NOT NULL DEFAULT '',
    revocation_request_sha256 TEXT NOT NULL DEFAULT '',
    revoked_audit_event_id TEXT NOT NULL DEFAULT '',
    CHECK (
        (revoked_at IS NULL AND revoked_by_user_id = '' AND revoked_idempotency_key = '' AND revocation_request_sha256 = '' AND revoked_audit_event_id = '')
        OR
        (revoked_at IS NOT NULL AND revoked_by_user_id <> '' AND revoked_idempotency_key <> '' AND revocation_request_sha256 <> '' AND revoked_audit_event_id <> '')
    )
);

CREATE UNIQUE INDEX IF NOT EXISTS conversation_agent_action_grants_create_idempotency_unique
    ON conversation_agent_action_grants (org_id, created_by_user_id, created_idempotency_key);

CREATE UNIQUE INDEX IF NOT EXISTS conversation_agent_action_grants_active_unique
    ON conversation_agent_action_grants (
        org_id, conversation_id, action_id, space_ref, subject_id,
        recipient_audience_ref, privacy_policy_ref
    )
    WHERE revoked_at IS NULL;

CREATE INDEX IF NOT EXISTS conversation_agent_action_grants_effect_lookup_idx
    ON conversation_agent_action_grants (
        org_id, conversation_id, action_id, space_ref, subject_id,
        recipient_audience_ref, privacy_policy_ref
    )
    WHERE revoked_at IS NULL;
