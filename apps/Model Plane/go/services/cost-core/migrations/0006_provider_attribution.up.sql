-- Preserve the serving provider/deployment alias stamped by model-gateway.
-- This supports Azure reconciliation and does not alter historical rows.
ALTER TABLE cost_entries
    ADD COLUMN IF NOT EXISTS provider_used text NOT NULL DEFAULT '';
