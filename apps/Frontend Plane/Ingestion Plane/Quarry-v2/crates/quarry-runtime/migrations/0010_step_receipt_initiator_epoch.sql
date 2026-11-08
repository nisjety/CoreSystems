-- Record who drove each browser action and the control epoch it was admitted under.
--
-- Nullable and not backfilled: a receipt written before this migration has no
-- initiator, and guessing one would turn an unknown into a claim. Old binaries keep
-- working against the new columns (they neither read nor write them).
ALTER TABLE quarry_step_receipts
    ADD COLUMN IF NOT EXISTS initiated_by TEXT
        CHECK (initiated_by IS NULL OR initiated_by IN ('human', 'agent', 'system'));

ALTER TABLE quarry_step_receipts
    ADD COLUMN IF NOT EXISTS control_epoch BIGINT
        CHECK (control_epoch IS NULL OR control_epoch >= 0);
