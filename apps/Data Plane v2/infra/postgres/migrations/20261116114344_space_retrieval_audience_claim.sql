-- WS17-S9: a Space-scoped retrieval trace records the Space it was authorised under and the
-- recipient audience (hash and revision) that the verified Control decision carried.
-- Additive and nullable: every existing run and every unscoped run stays NULL. Ids and
-- digests only, never content.
ALTER TABLE retrieval_runs
    ADD COLUMN IF NOT EXISTS space_ref TEXT,
    ADD COLUMN IF NOT EXISTS space_audience_hash TEXT,
    ADD COLUMN IF NOT EXISTS space_audience_revision BIGINT;
