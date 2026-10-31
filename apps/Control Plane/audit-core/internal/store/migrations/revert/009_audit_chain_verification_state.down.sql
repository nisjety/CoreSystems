-- Revert of 009_audit_chain_verification_state.sql. Run by hand (the service
-- applies up-migrations only), after the new image is stopped. Only the
-- verifier's own bookkeeping is lost: the next run starts a full verification
-- from the trust base. Also delete the schema_migrations row so a later
-- re-apply is possible:
--   DELETE FROM schema_migrations WHERE version = '009_audit_chain_verification_state';

DROP TABLE IF EXISTS audit_chain_verifications;
