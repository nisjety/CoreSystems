-- Record where the cost on a step receipt came from (WS6.B).
--
-- `flat_table`: the flat action-cost table (the estimate the budget gate charges).
-- `provider_billed`: a figure the driver reported. `estimated` is reserved for a
-- planner or other model-side estimate.
--
-- Nullable and not backfilled: a receipt written before this migration has no
-- recorded source, and guessing one would turn an unknown into a claim. Old
-- binaries keep working against the new column (they neither read nor write it).
ALTER TABLE quarry_step_receipts
    ADD COLUMN IF NOT EXISTS cost_source TEXT
        CHECK (cost_source IS NULL OR cost_source IN ('flat_table', 'estimated', 'provider_billed'));
