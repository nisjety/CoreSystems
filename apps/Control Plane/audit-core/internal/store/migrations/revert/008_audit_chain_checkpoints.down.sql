-- Revert of 008_audit_chain_checkpoints.sql. Run by hand (the service applies
-- up-migrations only), after AUDIT_CHAIN_PRUNE_ENABLED is 0 and the new image
-- is stopped. Dropping the checkpoints is safe only while no purge has run with
-- pruning on: once a chain has been pruned, its first remaining row links to a
-- checkpoint, and without the checkpoint the chain no longer verifies from
-- genesis. Also delete the schema_migrations row so a later re-apply is
-- possible:
--   DELETE FROM schema_migrations WHERE version = '008_audit_chain_checkpoints';

DROP INDEX IF EXISTS idx_audit_chain_incidents_org_created;
DROP TABLE IF EXISTS audit_chain_incidents;
DROP TABLE IF EXISTS audit_chain_attestations;
DROP TABLE IF EXISTS audit_chain_checkpoints;
