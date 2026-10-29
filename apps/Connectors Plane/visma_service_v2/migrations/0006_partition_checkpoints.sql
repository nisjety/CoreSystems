-- Durable per-partition sync checkpoints.
--
-- Existing endpoint-level checkpoints become the `default` partition. Runtime
-- ERP REST partitions then write their own rows keyed by partition_key.

ALTER TABLE vsv2.sync_checkpoint
    ADD COLUMN IF NOT EXISTS partition_key TEXT NOT NULL DEFAULT 'default';

ALTER TABLE vsv2.sync_checkpoint
    DROP CONSTRAINT IF EXISTS sync_checkpoint_pkey;

ALTER TABLE vsv2.sync_checkpoint
    ADD PRIMARY KEY (tenant_id, endpoint, source, partition_key);
