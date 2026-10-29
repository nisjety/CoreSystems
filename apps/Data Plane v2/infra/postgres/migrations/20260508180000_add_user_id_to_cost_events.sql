-- Wave 3.1 §15-F — attribute cost ledger to (user_id, org_id) instead of org_id only.
ALTER TABLE cost_events ADD COLUMN IF NOT EXISTS user_id TEXT;
CREATE INDEX IF NOT EXISTS idx_cost_events_user_created
    ON cost_events (user_id, created_at DESC)
    WHERE user_id IS NOT NULL;
