-- Learned skills as owner-scoped proposals (build plan WS07 S7).
--
-- A skill the learning review distils from a conversation is a proposal that
-- belongs to the person whose conversation it came from. It is written disabled
-- and user-scoped, and nothing injects it until its owner accepts it:
--
--   review_state     'proposed' (awaiting the owner), 'accepted' or 'rejected'. Every
--                    existing row reads 'accepted': nothing about today's skills changes
--   source_run_id    the run the proposal was distilled from (provenance, not content)
--   source_thread_id the thread of that run
--   proposal_reason  the reviewer's reason for proposing it, at most 300 characters
--
-- The unique key moves from (org_id, name) to (org_id, scope, owner_user_id, name):
-- with user-scoped rows two people can each have a skill of the same name, and under
-- the old key the second one's upsert would have overwritten the first one's private
-- skill. The new key is weaker than the old one for every existing row (org rows keep
-- owner_user_id = '' and so stay unique per org and name), so no data conflicts.
--
-- contract: drop agent_skills_org_name_uq; the unique index agent_skills_scope_owner_name_uq replaces it in this same migration and is implied by it for every existing row (an older session-core's ON CONFLICT (org_id, name) cannot run over this schema, so roll its image back only together with revert/0058_agent_skills_proposals.down.sql)
--
-- Content-free additions: ids, a state and one short sentence. Rows are erased with
-- their owner and with the organization (gdpr.rs).
ALTER TABLE agent_skills
    ADD COLUMN IF NOT EXISTS review_state TEXT NOT NULL DEFAULT 'accepted',
    ADD COLUMN IF NOT EXISTS source_run_id TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS source_thread_id TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS proposal_reason TEXT NOT NULL DEFAULT '';

-- NOT VALID first, VALIDATE second, as 0051 does: a validated CHECK scans the table
-- under ACCESS EXCLUSIVE. The guard keeps a re-run harmless.
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'agent_skills_review_state_check'
    ) THEN
        ALTER TABLE agent_skills
            ADD CONSTRAINT agent_skills_review_state_check
            CHECK (review_state IN ('proposed', 'accepted', 'rejected')) NOT VALID;
    END IF;
END
$$;
ALTER TABLE agent_skills VALIDATE CONSTRAINT agent_skills_review_state_check;

CREATE UNIQUE INDEX IF NOT EXISTS agent_skills_scope_owner_name_uq
    ON agent_skills (org_id, scope, owner_user_id, name);

DROP INDEX IF EXISTS agent_skills_org_name_uq;

-- The owner's inbox: only the proposals still waiting on a decision.
CREATE INDEX IF NOT EXISTS agent_skills_proposed_idx
    ON agent_skills (org_id, owner_user_id)
    WHERE review_state = 'proposed';
