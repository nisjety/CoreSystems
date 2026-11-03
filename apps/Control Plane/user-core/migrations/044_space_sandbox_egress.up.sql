-- 044_space_sandbox_egress: the org's sandbox egress ceiling for Space
-- capability decisions (EP-9 E2, F-t3-043).
--
-- sandbox_egress_tier is the highest egress tier Control will sign for a
-- sandbox lease in this org's Spaces. The closed vocabulary is
-- off|registries|allowlist|all, but 'all' is deliberately not storable in W3:
-- the CHECK below refuses it, so no org can be granted unrestricted network
-- reach from a sandbox. Default 'off' keeps every existing org where it is.
--
-- sandbox_egress_allowlist holds canonical host names only (no scheme, port,
-- path or IP literal), at most 32, sorted and de-duplicated by the service.
-- It carries no content, so Zero Data Retention needs no special handling
-- here; a ZDR Space never receives an egress permission at decision time.
--
-- Read only while SPACE_SANDBOX_EGRESS_TIERS is on; with the switch off the
-- columns exist and are ignored. Idempotent: every statement tolerates a
-- re-run.

BEGIN;

ALTER TABLE space_effect_policies
    ADD COLUMN IF NOT EXISTS sandbox_egress_tier TEXT NOT NULL DEFAULT 'off';
ALTER TABLE space_effect_policies
    ADD COLUMN IF NOT EXISTS sandbox_egress_allowlist JSONB NOT NULL DEFAULT '[]'::jsonb;

ALTER TABLE space_effect_policies DROP CONSTRAINT IF EXISTS space_effect_policies_sandbox_egress_tier_chk;
ALTER TABLE space_effect_policies
    ADD CONSTRAINT space_effect_policies_sandbox_egress_tier_chk
    CHECK (sandbox_egress_tier IN ('off', 'registries', 'allowlist'));

-- The service canonicalizes and caps the list; this is the backstop for a
-- writer that bypasses it.
ALTER TABLE space_effect_policies DROP CONSTRAINT IF EXISTS space_effect_policies_sandbox_egress_allowlist_chk;
ALTER TABLE space_effect_policies
    ADD CONSTRAINT space_effect_policies_sandbox_egress_allowlist_chk
    CHECK (jsonb_typeof(sandbox_egress_allowlist) = 'array'
           AND jsonb_array_length(sandbox_egress_allowlist) <= 32);

COMMIT;
