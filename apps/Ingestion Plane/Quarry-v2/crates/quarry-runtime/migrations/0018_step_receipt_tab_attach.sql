-- WS8.F: the browser tab and the attach a step ran under, on every step receipt.
--
-- `tab_id` is the tab the step addressed (a delegate step always has one; a person's step
-- has one when the run had more than one tab). `attach_id` is the agent attachment a
-- delegate step ran under. Both are nullable and not backfilled: a receipt written before
-- this migration has no tab or attach, and guessing one would turn an unknown into a claim.
-- Old binaries keep working against the new columns (they neither read nor write them).
-- Number 0018 is the queue ruling (W2 queue 0.1): 0012 is WS6.B, 0013 WS12.S3.
ALTER TABLE quarry_step_receipts
    ADD COLUMN IF NOT EXISTS tab_id TEXT,
    ADD COLUMN IF NOT EXISTS attach_id TEXT;

CREATE INDEX IF NOT EXISTS quarry_step_receipts_attach_idx
    ON quarry_step_receipts (org_id, run_id, attach_id, step ASC)
    WHERE attach_id IS NOT NULL;
