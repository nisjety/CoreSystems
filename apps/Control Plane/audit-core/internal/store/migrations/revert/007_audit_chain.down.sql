-- Revert of 007_audit_chain.sql. Run by hand (the service applies up-migrations
-- only), after AUDIT_CHAIN_ENABLED is 0 and the old image is not yet started:
-- the chain columns and head table are dropped, so every chained row becomes an
-- ordinary row again and the recorded chain is lost. Also delete the
-- schema_migrations row so a later re-apply is possible:
--   DELETE FROM schema_migrations WHERE version = '007_audit_chain';

DROP TRIGGER IF EXISTS audit_events_guard ON audit_events;
DROP FUNCTION IF EXISTS audit_events_guard();
DROP TABLE IF EXISTS audit_chain_heads;
DROP INDEX IF EXISTS uq_audit_events_org_chain;
ALTER TABLE audit_events DROP CONSTRAINT IF EXISTS audit_events_chain_mode_check;
ALTER TABLE audit_events
  DROP COLUMN IF EXISTS chain_mode,
  DROP COLUMN IF EXISTS chain_version,
  DROP COLUMN IF EXISTS row_hash,
  DROP COLUMN IF EXISTS prev_hash,
  DROP COLUMN IF EXISTS chain_seq;
