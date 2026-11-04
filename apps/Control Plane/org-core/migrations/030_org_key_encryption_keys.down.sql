-- Reverses 030. DESTRUCTIVE: dropping org_key_encryption_keys crypto-shreds every
-- organisation at once, so every DEK any caller wrapped through org-core becomes
-- unrecoverable (every oc:-sealed Quarry profile). Roll back only while no row
-- holds key material (SELECT count(*) FROM org_key_encryption_keys WHERE state <>
-- 'destroyed' returns 0) or after that loss is accepted. Undelivered audit intents
-- are lost too.
DROP TABLE IF EXISTS org_key_audit_outbox;
DROP TABLE IF EXISTS org_key_master_canaries;
DROP TABLE IF EXISTS org_key_encryption_keys;
