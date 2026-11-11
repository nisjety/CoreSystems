-- The provider connection a stored webhook event was bound to (W4-T6 trigger dry run, event-to-connection binding).
--
-- integration_webhook_events carried no connection id, so the trigger dry run in capability-core could not confirm that a
-- stored event came from a trigger's own connection. The ingest path knows the connection for the account-wide callbacks it
-- resolves (the Meta family and Slack, through webhookorg); it stores that id here. An event from any other provider keeps
-- NULL, and NULL means the event is bound to no connection, so the dry run refuses it.
--
-- Additive only: one nullable column with no default (a metadata-only change, no rewrite). Existing rows stay NULL. Nothing
-- is backfilled, because the connection of an event stored before this migration cannot be recovered from its row.
-- Forward-only, like the other migrations in this service.

ALTER TABLE integration_webhook_events
  ADD COLUMN IF NOT EXISTS connection_id TEXT NULL
    CHECK (connection_id IS NULL OR connection_id <> '');
