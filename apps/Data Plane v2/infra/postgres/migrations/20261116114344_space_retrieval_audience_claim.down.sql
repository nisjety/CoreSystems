ALTER TABLE retrieval_runs
    DROP COLUMN IF EXISTS space_audience_revision,
    DROP COLUMN IF EXISTS space_audience_hash,
    DROP COLUMN IF EXISTS space_ref;
