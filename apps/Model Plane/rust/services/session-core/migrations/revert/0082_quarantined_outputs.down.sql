-- Revert of 0082_quarantined_outputs (build plan WS10-S8).
DROP INDEX IF EXISTS quarantined_outputs_expiry_idx;
DROP INDEX IF EXISTS quarantined_outputs_release_idx;
DROP TABLE IF EXISTS quarantined_outputs;
