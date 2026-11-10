-- W3-E3.5-3c: a job's own webhook (params.webhook, W3-E3.5-3b) delivers its run events through
-- webhook_deliveries, with webhook_id = 'job:<job id>'. That id names no webhooks row, so the
-- foreign key from migration 002 would refuse every such row. The key is dropped here.
--
-- The subscription cascade is kept. Deleting a webhooks row (DeleteByOrg, the org purge) must still
-- take its delivery rows with it, as ON DELETE CASCADE did; the trigger below does that. No webhooks
-- row has a 'job:' id, so the trigger never touches a job's delivery rows.
--
-- Idempotent: IF EXISTS on the drop, CREATE OR REPLACE for the function, and DROP TRIGGER IF EXISTS
-- before CREATE TRIGGER, so a second application is a no-op.

ALTER TABLE webhook_deliveries DROP CONSTRAINT IF EXISTS webhook_deliveries_webhook_id_fkey;

CREATE OR REPLACE FUNCTION webhook_deliveries_cascade_subscription() RETURNS trigger AS $$
BEGIN
    DELETE FROM webhook_deliveries WHERE webhook_id = OLD.id;
    RETURN OLD;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS webhooks_cascade_deliveries ON webhooks;
CREATE TRIGGER webhooks_cascade_deliveries
    AFTER DELETE ON webhooks
    FOR EACH ROW EXECUTE FUNCTION webhook_deliveries_cascade_subscription();
