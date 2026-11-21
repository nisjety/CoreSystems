-- Reverses 036. Dropping browser_site_audit_outbox loses every audit intent not yet published, so
-- roll back only once those intents are delivered, or after that loss is accepted. The site policy,
-- the member choices and the history are not touched.
DROP TABLE IF EXISTS browser_site_audit_outbox;
