-- Revert WS7.H history. The rows are a record of past organisation configuration;
-- dropping them loses the audit trail of who changed the policy, so roll back only
-- together with 025.
DROP TABLE IF EXISTS organization_web_policy_history;
