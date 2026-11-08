-- WS05-S2: redact the payloads the former SCIM stub persisted.
--
-- Until this change `ALL /api/v1/scim/v2/*` answered success and stored the whole inbound
-- request body (user names, emails, externalIds, group members) in
-- integration_webhook_events with provider_key = 'scim'. The stub is now an honest 501 and
-- stores nothing. The rows are redacted rather than deleted: the event identity, event type,
-- organization and timestamp stay as an inert trace, only the personal data is dropped.
--
-- Forward-only (there are no down migrations in this service) and idempotent.
UPDATE integration_webhook_events
   SET payload = '{}'::jsonb
 WHERE provider_key = 'scim'
   AND payload <> '{}'::jsonb;
