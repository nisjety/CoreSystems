-- EP-9 E3 sandbox egress: the egress tier a lease permits, decided once at
-- acquire from the verified Control decision and kept on the row, as
-- processes_permitted is (0003). Idempotent: the entrypoint re-runs every
-- up.sql at each boot, with no ledger.
--
-- Hosts and paths are never stored here, only the tier word. 'all' is not
-- storable in Wave 3 (D-E3), so the check refuses it as well as any unknown
-- word. An existing row reads as off, which is what every lease granted so
-- far was.
ALTER TABLE leases ADD COLUMN IF NOT EXISTS egress_tier_permitted TEXT NOT NULL DEFAULT 'off';
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'leases_egress_tier_permitted_check'
    ) THEN
        ALTER TABLE leases ADD CONSTRAINT leases_egress_tier_permitted_check
            CHECK (egress_tier_permitted IN ('off', 'registries', 'allowlist'));
    END IF;
END
$$;
