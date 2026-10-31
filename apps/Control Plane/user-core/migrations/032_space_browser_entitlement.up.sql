-- A Space is not permitted to let its agents browse the web merely by existing.
-- This is a standing, deny-by-default floor (DEC-19 D9), independent of every
-- other Space entitlement: the organisation's site policy can allow a host, but
-- a Space this floor does not entitle is refused before any browser grant is
-- minted. It is a boolean only; the admin's per-site rules live in org-core.
ALTER TABLE space_effect_policies
    ADD COLUMN IF NOT EXISTS browser_entitled BOOLEAN NOT NULL DEFAULT FALSE;
