-- Reverse 20261110120000_wiki_create_page_proposals.sql.
--
-- Refuses to run while a create_page proposal exists. Such a row has no
-- page_id, so page_id cannot go back to NOT NULL until the row is resolved
-- (accepted, rejected or in conflict). This file never deletes proposal rows.
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM wiki_proposals WHERE proposal_kind = 'create_page') THEN
        RAISE EXCEPTION 'wiki_proposals holds create_page rows; resolve them before rolling back 20261110120000';
    END IF;
END
$$;

ALTER TABLE wiki_proposals
    DROP CONSTRAINT IF EXISTS wiki_proposals_kind_shape_check,
    DROP CONSTRAINT IF EXISTS wiki_proposals_kind_check,
    ALTER COLUMN page_id SET NOT NULL,
    DROP COLUMN IF EXISTS proposed_title,
    DROP COLUMN IF EXISTS proposed_path,
    DROP COLUMN IF EXISTS proposal_kind;
