-- Rollback for 20260508150000_add_mode_mix_and_graph_exports.sql

-- Drop graph_exports (no FK from other tables → safe drop).
DROP INDEX IF EXISTS idx_graph_exports_org_created;
DROP TABLE IF EXISTS graph_exports;

-- Drop the mode_mix column. Existing rows lose their per-query weight history.
ALTER TABLE retrieval_runs DROP COLUMN IF EXISTS mode_mix;

-- wiki_maintenance_logs columns were `IF NOT EXISTS`-added in the forward —
-- keep them; dropping them would corrupt rows the sweep endpoint wrote.
