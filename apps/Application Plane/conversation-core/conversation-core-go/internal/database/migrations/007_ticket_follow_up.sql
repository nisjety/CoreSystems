-- A shared operator follow-up is deliberately separate from the SLA deadline.
-- `due_at` drives SLA risk/breach calculations and must never be overloaded by
-- a planning reminder.
ALTER TABLE conversation_tickets
    ADD COLUMN IF NOT EXISTS follow_up_at TIMESTAMPTZ;

CREATE INDEX IF NOT EXISTS conversation_tickets_org_follow_up_idx
    ON conversation_tickets (org_id, follow_up_at ASC NULLS LAST)
    WHERE follow_up_at IS NOT NULL;
