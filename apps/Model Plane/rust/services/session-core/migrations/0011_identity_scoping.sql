-- Security boundary: user-owned durable state must never collide or read
-- across users that happen to share one organization.

DROP INDEX IF EXISTS idx_approvals_idempotency;
CREATE UNIQUE INDEX idx_approvals_idempotency
    ON approvals (org_id, user_id, idempotency_key)
    WHERE idempotency_key <> '';

DROP INDEX IF EXISTS agent_memory_org_scope_key_uq;
CREATE UNIQUE INDEX agent_memory_org_scope_key_uq
    ON agent_memory (org_id, scope, owner, key)
    WHERE session_id IS NULL;

CREATE INDEX IF NOT EXISTS agent_memory_user_owner_idx
    ON agent_memory (org_id, owner, updated_at DESC)
    WHERE session_id IS NULL AND scope = 'user';
