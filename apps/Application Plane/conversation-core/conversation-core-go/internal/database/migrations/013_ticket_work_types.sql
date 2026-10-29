-- Cases, internal work, and incidents share ticket mechanics but not meaning.
-- Keep the semantic type authoritative rather than inferring it from labels.
ALTER TABLE conversation_tickets
    ADD COLUMN IF NOT EXISTS work_type TEXT NOT NULL DEFAULT 'customer_case';

DO $$
BEGIN
    ALTER TABLE conversation_tickets
        ADD CONSTRAINT conversation_tickets_work_type_check
        CHECK (work_type IN ('customer_case', 'internal_work', 'incident'));
EXCEPTION
    WHEN duplicate_object THEN NULL;
END $$;

CREATE INDEX IF NOT EXISTS conversation_tickets_org_work_type_updated_idx
    ON conversation_tickets (org_id, work_type, updated_at DESC);
