-- Revert WS7.H audit outbox. Undelivered intents are lost; roll back only after the
-- outbox is drained or the loss of those events is accepted.
DROP TABLE IF EXISTS organization_web_policy_audit_outbox;
