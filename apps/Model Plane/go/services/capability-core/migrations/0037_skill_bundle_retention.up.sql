-- 0037_skill_bundle_retention.up.sql
--
-- Skill bundles kept per version (EP-11 S5). Reserved as 0034 in round B; 0034 and 0036 are
-- taken in the tree and 0035 is EP-5 S6's, so this is the next free number.
--
-- A bundle row gains a state:
--   live        the bundle a skill's turns read today (at most one per skill)
--   draft       an upload made while the skill has an unpublished draft (at most one per skill);
--               publishing the draft makes it live
--   superseded  a bundle a newer upload replaced, kept while a version of the skill names its
--               manifest digest (agent_skill_versions.bundle_sha256), so a run pinned to that
--               version still reads its own files
-- Only capability-core's SKILL_BUNDLE_RETENTION writes draft or superseded. With it off, an
-- upload deletes the skill's bundles as before and every row stays live, so the reads (which
-- now ask for state = 'live') answer exactly as they did.
--
-- RE-RUN SAFETY. capability-core re-applies every *.up.sql at every boot, in name order, so
-- 0021 runs again before this file. 0021 creates skill_bundles_org_skill_uq on (org_id,
-- skill_id) with IF NOT EXISTS, which matches by NAME: this file therefore keeps the name and
-- swaps the index for its partial form (live rows only). On every later boot 0021 finds the
-- name and does nothing, so a retained superseded row can never make the boot fail.
-- Everything else here is idempotent too.
--
-- contract: skill_bundles_org_skill_uq narrows from every row to live rows (an older capability-core whose Put deletes all of a skill's bundles still runs over this schema; roll back with 0037_skill_bundle_retention.down.sql, which deletes the kept rows first)
--
-- Content: retained bundles are org configuration (D9), never conversation content; a ZDR
-- principal cannot upload one (authz.go refuses its writes). They go with their skill: a skill
-- delete removes every bundle of the skill while the switch is on.

ALTER TABLE skill_bundles
    ADD COLUMN IF NOT EXISTS state TEXT NOT NULL DEFAULT 'live';

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'skill_bundles_state_check'
    ) THEN
        ALTER TABLE skill_bundles
            ADD CONSTRAINT skill_bundles_state_check
            CHECK (state IN ('live', 'draft', 'superseded'));
    END IF;
    -- Swap the full unique index for the partial one under the same name (see RE-RUN SAFETY).
    IF EXISTS (
        SELECT 1 FROM pg_indexes
        WHERE indexname = 'skill_bundles_org_skill_uq' AND indexdef NOT LIKE '%WHERE%'
    ) THEN
        DROP INDEX skill_bundles_org_skill_uq;
    END IF;
END
$$;

CREATE UNIQUE INDEX IF NOT EXISTS skill_bundles_org_skill_uq
    ON skill_bundles (org_id, skill_id) WHERE state = 'live';

CREATE UNIQUE INDEX IF NOT EXISTS skill_bundles_org_skill_draft_uq
    ON skill_bundles (org_id, skill_id) WHERE state = 'draft';

-- One row per manifest of a skill: an upload of bytes the skill already holds flips that row's
-- state instead of storing them twice.
CREATE UNIQUE INDEX IF NOT EXISTS skill_bundles_org_skill_manifest_uq
    ON skill_bundles (org_id, skill_id, manifest_sha256);
