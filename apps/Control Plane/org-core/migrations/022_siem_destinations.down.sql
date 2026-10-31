-- Revert WS06-S7. The destination table holds organisation configuration (a URL,
-- a sealed secret and a filter) that an admin can re-enter, and nothing reads it
-- until ORG_SIEM_DESTINATIONS_ENABLED is switched on. The audit outbox rows are
-- dropped with it: a change intent not yet published is lost, so drain the
-- outbox (published_at set on every row) before reverting if the trail matters.
DROP TABLE IF EXISTS organization_siem_audit_outbox;
DROP TRIGGER IF EXISTS siem_destinations_limit ON siem_destinations;
DROP TABLE IF EXISTS siem_destinations;
DROP FUNCTION IF EXISTS siem_destinations_enforce_limit();
