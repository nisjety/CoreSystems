-- A Space is not permitted to acquire a sandbox lease merely by existing.
-- This is a standing, deny-by-default floor, independent of thread creation
-- or any other Space entitlement: a Space that can chat must not
-- automatically get its agents an execution sandbox.
ALTER TABLE space_effect_policies
    ADD COLUMN IF NOT EXISTS sandbox_capability_entitled BOOLEAN NOT NULL DEFAULT FALSE;
