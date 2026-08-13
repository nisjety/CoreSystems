-- A schedule creation is not permission to keep acting forever. The scheduler
-- must obtain a fresh Control decision at each fire, and that decision is
-- denied unless this independent entitlement is explicitly enabled.
ALTER TABLE space_effect_policies
    ADD COLUMN IF NOT EXISTS schedule_fire_entitled BOOLEAN NOT NULL DEFAULT FALSE;
