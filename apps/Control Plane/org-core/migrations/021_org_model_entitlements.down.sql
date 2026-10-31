-- Revert WS10-S2. The tables hold only organisation configuration (model ids and
-- actor ids), and nothing reads them until inference-core is switched on (WS10
-- S3), so dropping them loses a policy an admin can re-enter and nothing else.
DROP TABLE IF EXISTS org_model_policy_history;
DROP TABLE IF EXISTS org_model_policy_revisions;
DROP TABLE IF EXISTS org_model_policies;
