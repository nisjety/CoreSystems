-- 0006_agent_skills_origin.sql
-- G7 closed learning loop (docs/capability-ownership-matrix.md §G7).
--
-- Adds provenance to agent_skills so the auto-curator can never overwrite a
-- human-authored skill with a machine-proposed one. Existing rows default to
-- 'user' (treated as protected) — the safe assumption for anything written
-- before provenance existed.
--
-- Additive + idempotent: safe to apply to a live table.

ALTER TABLE agent_skills
    ADD COLUMN IF NOT EXISTS origin TEXT NOT NULL DEFAULT 'user';

-- Constrain to the known provenance values; mirrors learning.Origin in
-- capability-core/internal/learning.
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'agent_skills_origin_chk'
    ) THEN
        ALTER TABLE agent_skills
            ADD CONSTRAINT agent_skills_origin_chk
            CHECK (origin IN ('user', 'background_review'));
    END IF;
END$$;
