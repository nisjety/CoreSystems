-- Canonical tenant ownership and replay/privacy controls for booking data.
-- Historical rows cannot be attributed safely, so they are quarantined under
-- a sentinel that no Auth Core token may issue.

ALTER TABLE bookings ADD COLUMN org_id text;
UPDATE bookings SET org_id = 'legacy-unscoped' WHERE org_id IS NULL;
ALTER TABLE bookings ALTER COLUMN org_id SET NOT NULL;

ALTER TABLE bookings ADD COLUMN actor_principal_type text;
UPDATE bookings SET actor_principal_type = 'legacy' WHERE actor_principal_type IS NULL;
ALTER TABLE bookings ALTER COLUMN actor_principal_type SET NOT NULL;

ALTER TABLE bookings
    ADD COLUMN approval_id text,
    ADD COLUMN idempotency_key text,
    ADD COLUMN request_digest text,
    ADD COLUMN zdr boolean NOT NULL DEFAULT false,
    ADD COLUMN retention_until timestamptz,
    ADD COLUMN confirmation_expires_at timestamptz;

CREATE INDEX idx_bookings_org_created ON bookings(org_id, created_at DESC);
CREATE UNIQUE INDEX idx_bookings_org_idempotency
    ON bookings(org_id, idempotency_key)
    WHERE idempotency_key IS NOT NULL;

ALTER TABLE manifests ADD COLUMN org_id text;
UPDATE manifests SET org_id = 'legacy-unscoped' WHERE org_id IS NULL;
ALTER TABLE manifests ALTER COLUMN org_id SET NOT NULL;
ALTER TABLE manifests DROP CONSTRAINT manifests_carrier_code_manifest_date_key;
ALTER TABLE manifests
    ADD CONSTRAINT manifests_org_carrier_date_key
    UNIQUE (org_id, carrier_code, manifest_date);

CREATE INDEX idx_manifests_org_created ON manifests(org_id, created_at DESC);
