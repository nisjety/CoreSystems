-- 0111: the Space scope of agent_skills (Wave 3 EP-11 S9).
--
-- Reserved as 0099 in round B; numbered 0111 so it applies after 0109, which adds the
-- scope_id column this file constrains (on a fresh database 0099 would run first and fail on
-- the missing column). 0110 is left to EP-11 S5, whose reserved 0097 has the same need.
--
-- A Space skill (scope 'space') steers only the turns of threads inside its Space; scope_id
-- names that Space. The CHECK pair makes the two columns one fact: a Space row always names
-- its Space, and no org or user row ever does, so a Space id can never widen an org or user
-- skill and a Space row without a Space cannot exist. A Space row has no owner: the Space
-- holds it (its author is the revision's actor), so no person's own-skill read can select it.
--
-- Who may write a Space row is not decided here. capability-core refuses every Space write
-- until Control signs a "Space skill write" decision (EP-11 D7), and session-core lists Space
-- rows only for a thread it resolves to that Space itself (SKILL_SPACE_SCOPE, default off). No
-- writer creates one today, so this file changes no existing row and no read.
--
-- Additive: an older session-core never writes 'space'. Its list does not know the scope,
-- though, and a service caller listing with an empty user would match a Space row's empty
-- owner, so roll session-core back over Space rows only together with
-- revert/0111_agent_skills_space_scope.down.sql, which deletes them and restores the
-- two-scope CHECK. While no Space row exists (every switch off) the older image is safe.
--
-- Content: a Space skill is org configuration, like any skill, erased with the skill, with
-- the organization (gdpr.rs deletes every agent_skills row of the org) and by owner. Zero
-- Data Retention: ZDR credentials cannot write agent_skills at either writer.

-- NOT VALID first, VALIDATE second, as 0058 and 0109 do: a validated CHECK scans the table
-- under ACCESS EXCLUSIVE. Dropping the old CHECK and adding the wider one in the same
-- transaction leaves no moment without a scope CHECK.
ALTER TABLE agent_skills DROP CONSTRAINT IF EXISTS agent_skills_scope_chk;
ALTER TABLE agent_skills
    ADD CONSTRAINT agent_skills_scope_chk
    CHECK (scope IN ('org', 'user', 'space')) NOT VALID;
ALTER TABLE agent_skills VALIDATE CONSTRAINT agent_skills_scope_chk;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'agent_skills_scope_id_chk'
    ) THEN
        ALTER TABLE agent_skills
            ADD CONSTRAINT agent_skills_scope_id_chk
            CHECK ((scope = 'space') = (scope_id <> '')) NOT VALID;
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'agent_skills_space_owner_chk'
    ) THEN
        ALTER TABLE agent_skills
            ADD CONSTRAINT agent_skills_space_owner_chk
            CHECK (scope <> 'space' OR owner_user_id = '') NOT VALID;
    END IF;
END
$$;
ALTER TABLE agent_skills VALIDATE CONSTRAINT agent_skills_scope_id_chk;
ALTER TABLE agent_skills VALIDATE CONSTRAINT agent_skills_space_owner_chk;

-- The Space list reads one Space's rows of one org; partial, so it costs nothing while no
-- Space row exists.
CREATE INDEX IF NOT EXISTS agent_skills_space_idx
    ON agent_skills (org_id, scope_id)
    WHERE scope = 'space';
