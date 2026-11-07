-- Reverses 0037_skill_bundle_retention.up.sql.
--
-- DESTROYS every draft and superseded bundle (their files cascade) and every blob only they
-- named. A run pinned to an older skill version then reads no bundle for it. The live bundles
-- stay. Turn SKILL_BUNDLE_RETENTION off and roll capability-core back to a build that does not
-- read the state column first; with the switch off nothing writes a draft or superseded row.
--
-- The full (org_id, skill_id) unique index of 0021 comes back under its own name, so 0021's
-- re-run at boot stays a no-op.

DELETE FROM skill_bundles WHERE state <> 'live';

DELETE FROM skill_bundle_blobs b
WHERE NOT EXISTS (
    SELECT 1 FROM skill_bundle_files f
    JOIN skill_bundles sb ON sb.id = f.bundle_id
    WHERE sb.org_id = b.org_id AND f.sha256 = b.sha256
);

DROP INDEX IF EXISTS skill_bundles_org_skill_manifest_uq;
DROP INDEX IF EXISTS skill_bundles_org_skill_draft_uq;
DROP INDEX IF EXISTS skill_bundles_org_skill_uq;
CREATE UNIQUE INDEX IF NOT EXISTS skill_bundles_org_skill_uq
    ON skill_bundles (org_id, skill_id);

ALTER TABLE skill_bundles DROP CONSTRAINT IF EXISTS skill_bundles_state_check;
ALTER TABLE skill_bundles DROP COLUMN IF EXISTS state;
