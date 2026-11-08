ALTER TABLE leases DROP CONSTRAINT IF EXISTS leases_egress_tier_permitted_check;
ALTER TABLE leases DROP COLUMN IF EXISTS egress_tier_permitted;
