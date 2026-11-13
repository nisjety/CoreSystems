-- 0131: the revision snapshot carries a skill's url_patterns (Quarry follow-up QF-19).
--
-- 0130 added agent_skills.url_patterns but left the revision snapshot (0064, 0109, 0110) as it
-- was, so an undo (session-core RESTORE_SKILL_SQL, which writes a skill back from a revision
-- snapshot) had no patterns to write back and a restored skill came back with '[]'. This file
-- replaces 0110's snapshot with the same keys plus `url_patterns` last, always present (an empty
-- list is written as '[]'), so a snapshot that has the key is authoritative and one written before
-- this file (without the key) is recognisable. The restore keeps the row's current patterns for a
-- snapshot without the key, and writes '[]' only when it has to recreate a deleted skill from one.
--
-- The truncated shape (a skill over 64 KiB) is unchanged: the restore refuses a truncated snapshot.
--
-- Revisions: an edit that changes only url_patterns now records an `edited` revision (before, the
-- two snapshots were equal and nothing was recorded). Versions and digests are untouched: this
-- file does not replace agent_skill_version_body.
--
-- Content: URL patterns are org configuration, like the skill text the snapshot already carries,
-- and are erased with the revisions (skill delete, org purge in gdpr.rs, owner erasure). Zero Data
-- Retention: ZDR credentials cannot write agent_skills, so no ZDR content reaches a revision.

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
        'has_draft', s.draft IS NOT NULL,
        'url_patterns', s.url_patterns
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
