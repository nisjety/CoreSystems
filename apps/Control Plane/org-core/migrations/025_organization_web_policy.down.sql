-- Revert WS7.H storage. Nothing reads this table until WS7.I (auth-core claim
-- minting) is switched on, so dropping it loses only organisation configuration an
-- admin can re-enter.
DROP TABLE IF EXISTS organization_web_policy;
