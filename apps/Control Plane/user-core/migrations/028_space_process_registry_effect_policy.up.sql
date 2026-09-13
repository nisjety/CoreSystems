-- A Space entitled to an execution sandbox is not thereby entitled to leave
-- background processes running in it. Acquiring a lease is bounded work a
-- caller waits on; a background process keeps running after the turn that
-- started it, holds its own output, and can be reattached to later — a
-- different thing to authorize, so it gets its own standing deny-by-default
-- floor rather than inheriting sandbox_capability_entitled.
--
-- See apps/Frontend Plane/verevonv3/docs/S4_2_PROCESS_REGISTRY_DESIGN_2026-09-13.md §4.
ALTER TABLE space_effect_policies
    ADD COLUMN IF NOT EXISTS process_registry_entitled BOOLEAN NOT NULL DEFAULT FALSE;
