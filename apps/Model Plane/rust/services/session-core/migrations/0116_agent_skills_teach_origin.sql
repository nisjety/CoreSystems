-- 0116: the 'teach' origin of agent_skills (Teach mode, F-t3-081; W5-081-b).
--
-- A skill a person teaches the agent is filed as an owner-held proposal, the same shape as a learned skill
-- from the background review: user-scoped, owned by the person, disabled, and review_state 'proposed' until
-- the owner accepts it in the learning inbox. upsert_agent_skill treats origin 'teach' as it treats
-- 'background_review', except that a zero-retention or waived credential is refused, not skipped. This file
-- adds only the value itself.
--
-- The origin CHECK (0006) names every value a row may hold, and the WS07 proposal contract requires whoever
-- adds an origin to extend it (docs/plans/model-plane-chat-build-plan-2026-10-14.md, WS07). This file drops
-- that CHECK and re-adds it with 'teach' as a third value. It mirrors learning.Origin in
-- capability-core/internal/learning.
--
-- Additive: every value accepted before is still accepted, and no row is rewritten. A session-core that
-- predates this file never writes 'teach', so it runs unchanged against the wider CHECK. The reversal is in
-- revert/0116_agent_skills_teach_origin.down.sql.
--
-- Zero Data Retention: nothing new is stored. The origin is a closed token on a row that already exists, and
-- session-core refuses a zero-retention or waived credential's teach write before anything is read or written.

-- NOT VALID first, VALIDATE second, as 0111 does: a validated CHECK scans the table under ACCESS EXCLUSIVE.
ALTER TABLE agent_skills DROP CONSTRAINT IF EXISTS agent_skills_origin_chk;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'agent_skills_origin_chk'
    ) THEN
        ALTER TABLE agent_skills
            ADD CONSTRAINT agent_skills_origin_chk
            CHECK (origin IN ('user', 'background_review', 'teach')) NOT VALID;
    END IF;
END$$;

ALTER TABLE agent_skills VALIDATE CONSTRAINT agent_skills_origin_chk;
