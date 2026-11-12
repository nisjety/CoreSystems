-- browser-broker 0008: the grant web-policy snapshot (WS7.K, Q2-WS7.K-c).
--
-- R-3 reserved 0002 for WS7. This file is 0008 instead: 0007 is the fill_otp
-- binding (Q3-E3.1-b2-c), and a version-tracking migrator such as golang-migrate
-- does not apply a version below the already-applied 0006.
--
-- The container entrypoint re-applies every *.up.sql on each start, so this
-- statement is idempotent: ADD COLUMN IF NOT EXISTS skips it once it exists.
--
-- What changes:
--   * web_policy   the JSON of the grant's org web-policy snapshot: the verified
--                  token's web_policy claim, mapped 1:1 (internal/grant/policy.go).
--                  NULL when the grant carries no snapshot: a row written before
--                  this column, or a grant minted with none (the switch
--                  BROWSER_BROKER_WEB_POLICY_SNAPSHOT off, or a token with no
--                  web_policy claim). Session, API-key and human_browse grants
--                  take it from the token; run, attach and origin_extension
--                  children copy their parent's, in both stores.
--
-- Still no zdr column: a ZDR grant lives in the broker's memory with a "z_" id
-- and never reaches this table (X-2), so no ZDR snapshot is stored here.

ALTER TABLE browser_grants
    ADD COLUMN IF NOT EXISTS web_policy JSONB;
