-- 0110: a draft beside the live text of a skill (Wave 3 EP-11 S5).
--
-- Reserved as 0097 in round B; numbered 0110 so it applies after 0108 (versions) and 0109
-- (authored kinds), whose revision snapshot function this file replaces (on a fresh database
-- 0097 would run first and 0109 would put its own snapshot back without the draft marker).
--
-- With capability-core's SKILL_DRAFT_PUBLISH on, an edit of an accepted skill's text writes
-- `draft` and leaves the live fields alone, so turns keep using the published version until its
-- author publishes. Publishing copies the draft into the live fields and clears it in one
-- statement; 0108's trigger then cuts the new version. A restore writes an old version's body
-- into `draft`, so a restore is never live until it is published as a new version (EP-11 D2).
-- With the switch off nothing writes these columns and every row keeps draft NULL.
--
--   draft             the edited body, the same field set as agent_skill_version_body (name,
--                     description, content, trigger_keywords, trigger_file_patterns,
--                     tool_restrictions, kind, settings, bundle_sha256). NULL when the skill has
--                     no draft. Draft writes touch no versioned field, so they cut no version.
--   draft_updated_at  when the draft was last written; NULL with no draft.
--   draft_by          the person who last wrote the draft ('' with no draft).
--
-- Revisions: the snapshot (0064, as 0109 left it) gains `has_draft`, a boolean, never the draft
-- text, so starting, publishing or discarding a draft records an `edited` revision while a
-- revision stays bounded by the live text it already carried. Saving a draft over a draft records
-- nothing: the live row a person sees has not changed. Snapshots written before this file lack
-- the key, and the restore (session-core RESTORE_SKILL_SQL) never reads it, so an undo restores
-- the live text and leaves any draft as it is.
--
-- Content: a draft is authored skill text, the same class as the live row, and goes with it (skill
-- delete, org purge in gdpr.rs, owner erasure through owner_user_id). Zero Data Retention: ZDR
-- credentials cannot write agent_skills at either writer, so no ZDR content reaches a draft.

ALTER TABLE agent_skills
    ADD COLUMN IF NOT EXISTS draft JSONB,
    ADD COLUMN IF NOT EXISTS draft_updated_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS draft_by TEXT NOT NULL DEFAULT '';

-- A draft is an object or absent; publish reads its keys and a scalar would publish nothing.
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'agent_skills_draft_object_check'
    ) THEN
        ALTER TABLE agent_skills
            ADD CONSTRAINT agent_skills_draft_object_check
            CHECK (draft IS NULL OR jsonb_typeof(draft) = 'object') NOT VALID;
    END IF;
END
$$;
ALTER TABLE agent_skills VALIDATE CONSTRAINT agent_skills_draft_object_check;

-- 0109's snapshot with has_draft added last, in both the full and the truncated shape, so a draft
-- starting or ending on a large skill is still a visible change.
CREATE OR REPLACE FUNCTION agent_skill_revision_snapshot(s agent_skills) RETURNS JSONB AS $$
DECLARE
    snapshot JSONB;
BEGIN
    snapshot := jsonb_build_object(
        'id', s.id,
        'org_id', s.org_id,
        'name', s.name,
        'description', s.description,
        'content', s.content,
        'trigger_keywords', s.trigger_keywords,
        'trigger_file_patterns', s.trigger_file_patterns,
        'tool_restrictions', s.tool_restrictions,
        'enabled', s.enabled,
        'origin', s.origin,
        'scope', s.scope,
        'owner_user_id', s.owner_user_id,
        'shared_with', s.shared_with,
        'review_state', s.review_state,
        'source_run_id', s.source_run_id,
        'source_thread_id', s.source_thread_id,
        'proposal_reason', s.proposal_reason,
        'kind', s.kind,
        'settings', s.settings,
        'has_draft', s.draft IS NOT NULL
    );
    IF pg_column_size(snapshot) > 65536 THEN
        RETURN jsonb_build_object(
            'truncated', true,
            'id', s.id,
            'org_id', s.org_id,
            'name', s.name,
            'enabled', s.enabled,
            'scope', s.scope,
            'owner_user_id', s.owner_user_id,
            'review_state', s.review_state,
            'kind', s.kind,
            'has_draft', s.draft IS NOT NULL
        );
    END IF;
    RETURN snapshot;
END
$$ LANGUAGE plpgsql IMMUTABLE;
