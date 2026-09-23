-- Cache-token telemetry (native-compaction migration prerequisite):
-- additive, rollback-compatible columns for prompt-cache legs already
-- folded into input_tokens by the serving adapter. Existing binaries ignore
-- these columns; new binaries populate them from the USAGE_ENVELOPE payload
-- (0 for a request that reported no cache usage). Existing rows are
-- backfilled to 0 by the column default, which is the correct historical
-- value: nothing before this migration could have reported cache usage.
ALTER TABLE cost_entries
    ADD COLUMN IF NOT EXISTS cache_read_input_tokens bigint NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS cache_creation_input_tokens bigint NOT NULL DEFAULT 0;
