-- Cost basis: how an entry's cost_usd was established, from the
-- USAGE_ENVELOPE payload's `cost_basis` (model-gateway pricing::CostBasis).
--   'metered'      an API route, priced per token from model_pricing.
--   'subscription' served by the user's own ChatGPT subscription: cost_usd is
--                  0 by contract and the token counts are usage, not spend.
--   NULL           not reported (every row written before this migration, and
--                  any producer that predates the field).
-- This lets a dashboard tell subscription usage from free usage: both have
-- cost_usd = 0, and only this column separates them.
--
-- Additive and rollback-compatible, like 0003/0004: nullable with no default,
-- so existing rows stay NULL ("unknown", which is the true historical value)
-- and the previous binary ignores the column. Re-run on every boot by the
-- container entrypoint, hence IF NOT EXISTS.
ALTER TABLE cost_entries
    ADD COLUMN IF NOT EXISTS cost_basis text;
