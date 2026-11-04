-- Reverses 031. Dropping org_legal_holds releases every legal hold at once and
-- loses the record of what was held, and dropping org_retention_history loses who
-- set each window and placed or released each hold. Roll back only while no hold
-- is active (SELECT count(*) FROM org_legal_holds WHERE released_at IS NULL returns
-- 0) and no retention sweeper reads the policy (SESSION_RETENTION_SWEEP=off), or
-- after that loss is accepted. Undelivered audit intents are lost too.
DROP TABLE IF EXISTS org_retention_audit_outbox;
DROP TABLE IF EXISTS org_retention_history;
DROP TABLE IF EXISTS org_legal_holds;
DROP TABLE IF EXISTS org_retention_policies;
