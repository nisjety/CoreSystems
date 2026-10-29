-- 003: outbound-send audit + defense-in-depth send dedup for the HITL act-leg.
--
-- The primary no-double-send guard is the atomic approved->executed claim on
-- conversation_ai_actions (MarkAIActionExecuted). This table records every
-- attempted/completed send for audit, and its UNIQUE (org_id, ai_action_id)
-- constraint doubles as a second-layer dedup: a redelivered approve that somehow
-- got past the status claim still cannot insert a duplicate completed send row.
--
-- Org-scoped by org_id; ai_action_id == approval_id, so every send is auditable
-- by the approval that authorized it.

CREATE TABLE IF NOT EXISTS conversation_ai_action_sends (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL,
    ai_action_id TEXT NOT NULL,
    connection_id TEXT NOT NULL DEFAULT '',
    provider TEXT NOT NULL DEFAULT '',
    provider_message_id TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'sent',
    error TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT conversation_ai_action_sends_unique UNIQUE (org_id, ai_action_id)
);

CREATE INDEX IF NOT EXISTS idx_conversation_ai_action_sends_org_action
    ON conversation_ai_action_sends (org_id, ai_action_id);
