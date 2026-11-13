-- Reverse 20261110120000_wiki_create_page_proposals.sql.
--
-- Refuses to run while an unresolved create_page proposal exists (proposal_status 'pending', or
-- NULL). Resolved create_page rows ('accepted', 'rejected' or 'conflict') are deleted first, and
-- permanently: they keep page_id NULL, so page_id cannot go back to NOT NULL while they exist.
-- Edit proposals are not touched.
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM wiki_proposals
        WHERE proposal_kind = 'create_page'
          AND (proposal_status IS NULL OR proposal_status NOT IN ('accepted', 'rejected', 'conflict'))
    ) THEN
        RAISE EXCEPTION 'wiki_proposals holds unresolved create_page rows; resolve them before rolling back 20261110120000';
    END IF;
END
$$;

DELETE FROM wiki_proposals
 WHERE proposal_kind = 'create_page'
   AND proposal_status IN ('accepted', 'rejected', 'conflict');

ALTER TABLE wiki_proposals
    DROP CONSTRAINT IF EXISTS wiki_proposals_kind_shape_check,
    DROP CONSTRAINT IF EXISTS wiki_proposals_kind_check,
    ALTER COLUMN page_id SET NOT NULL,
    DROP COLUMN IF EXISTS proposed_title,
    DROP COLUMN IF EXISTS proposed_path,
    DROP COLUMN IF EXISTS proposal_kind;
