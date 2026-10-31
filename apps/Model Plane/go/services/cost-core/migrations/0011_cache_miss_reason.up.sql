-- Advisory prompt-cache miss reason (build plan WS02-S2): one of the closed set
-- below_minimum, prefix_changed, ttl_expired, no_baseline, unexplained, or NULL
-- when the producer did not classify the request (a hit, an older gateway, a
-- ZDR or subscription turn). Nullable and additive: a previous binary ignores
-- the column, and NULL on every existing row is the true historical value.
ALTER TABLE cost_entries
    ADD COLUMN IF NOT EXISTS cache_miss_reason text;
