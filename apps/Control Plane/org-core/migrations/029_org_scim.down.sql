-- Reverses 029. SCIM user records are a cache of what SCIM did to auth-core's
-- member table; dropping them does not change any membership. Tokens are lost,
-- so every identity provider must be given a new one after a rollback.
DROP TABLE IF EXISTS organization_scim_users;
DROP TABLE IF EXISTS organization_scim_tokens;
