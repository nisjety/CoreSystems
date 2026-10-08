-- Reverse 20261003140000_cost_events_v2.sql.
--
-- Rows are not deleted. The version 2 columns are dropped with their values (the exact
-- tokens, the feature, the provider, the forwarding state), so a version 2 row is left as a
-- count and a model, which is all a row was before this migration. That loses what the
-- ledger recorded since it was applied: take a copy of cost_events first if it matters.
-- Re-applying the forward migration does not bring it back; it cannot know what was
-- forwarded to cost-core, and a row it finds with no `forwarded_at` is a version 1 row,
-- which is never forwarded.
--
-- Order matters: constraints and indexes go before the columns they reference.

-- DROP COLUMN needs an exclusive lock on cost_events. Give up after 10 s instead of queueing
-- behind a long transaction and blocking every other reader and writer (see the forward
-- migration).
SET LOCAL lock_timeout = '10s';

DROP INDEX IF EXISTS idx_cost_events_forward_queue;
DROP INDEX IF EXISTS idx_cost_events_org_feature_created;

ALTER TABLE cost_events DROP CONSTRAINT IF EXISTS cost_events_schema_version_chk;
ALTER TABLE cost_events DROP CONSTRAINT IF EXISTS cost_events_v2_complete_chk;
ALTER TABLE cost_events DROP CONSTRAINT IF EXISTS cost_events_counts_chk;
ALTER TABLE cost_events DROP CONSTRAINT IF EXISTS cost_events_cost_basis_chk;
ALTER TABLE cost_events DROP CONSTRAINT IF EXISTS cost_events_feature_chk;
ALTER TABLE cost_events DROP CONSTRAINT IF EXISTS cost_events_zdr_counters_only_chk;
ALTER TABLE cost_events DROP CONSTRAINT IF EXISTS cost_events_forward_state_chk;

COMMENT ON COLUMN cost_events.estimated_tokens IS NULL;

ALTER TABLE cost_events
    DROP COLUMN IF EXISTS schema_version,
    DROP COLUMN IF EXISTS feature,
    DROP COLUMN IF EXISTS provider_used,
    DROP COLUMN IF EXISTS input_tokens,
    DROP COLUMN IF EXISTS output_tokens,
    DROP COLUMN IF EXISTS cache_read_tokens,
    DROP COLUMN IF EXISTS units,
    DROP COLUMN IF EXISTS cost_basis,
    DROP COLUMN IF EXISTS estimated,
    DROP COLUMN IF EXISTS zdr,
    DROP COLUMN IF EXISTS request_id,
    DROP COLUMN IF EXISTS forwarded_at,
    DROP COLUMN IF EXISTS forward_attempts,
    DROP COLUMN IF EXISTS forward_next_at,
    DROP COLUMN IF EXISTS forward_lease_until,
    DROP COLUMN IF EXISTS forward_error;
