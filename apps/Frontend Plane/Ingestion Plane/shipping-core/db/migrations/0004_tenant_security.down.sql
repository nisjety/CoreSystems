DROP INDEX IF EXISTS idx_manifests_org_created;
ALTER TABLE manifests DROP CONSTRAINT IF EXISTS manifests_org_carrier_date_key;
ALTER TABLE manifests ADD CONSTRAINT manifests_carrier_code_manifest_date_key UNIQUE (carrier_code, manifest_date);
ALTER TABLE manifests DROP COLUMN IF EXISTS org_id;

DROP INDEX IF EXISTS idx_bookings_org_idempotency;
DROP INDEX IF EXISTS idx_bookings_org_created;
ALTER TABLE bookings
    DROP COLUMN IF EXISTS confirmation_expires_at,
    DROP COLUMN IF EXISTS retention_until,
    DROP COLUMN IF EXISTS zdr,
    DROP COLUMN IF EXISTS request_digest,
    DROP COLUMN IF EXISTS idempotency_key,
    DROP COLUMN IF EXISTS approval_id,
    DROP COLUMN IF EXISTS actor_principal_type,
    DROP COLUMN IF EXISTS org_id;
