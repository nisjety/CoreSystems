-- W5-094-a: create-page proposals.
--
-- An agent may propose a page that does not exist yet. Such a row has no page
-- to hang from, so it carries the proposed path and title and a NULL page_id.
-- Accepting it writes the page through the same insert the direct create uses;
-- a live page already at the path turns the proposal into 'conflict' and
-- writes nothing.
--
-- Existing rows keep proposal_kind 'edit' (the DEFAULT) and their page_id. The
-- CHECK fixes the shape of each kind: an edit names a page, a create_page names
-- a path and no page. The RLS policy is unchanged; org_id still scopes the row.
ALTER TABLE wiki_proposals
    ADD COLUMN IF NOT EXISTS proposal_kind TEXT NOT NULL DEFAULT 'edit',
    ADD COLUMN IF NOT EXISTS proposed_path TEXT,
    ADD COLUMN IF NOT EXISTS proposed_title TEXT,
    ALTER COLUMN page_id DROP NOT NULL,
    ADD CONSTRAINT wiki_proposals_kind_check
        CHECK (proposal_kind IN ('edit', 'create_page')),
    ADD CONSTRAINT wiki_proposals_kind_shape_check CHECK (
        (proposal_kind = 'edit' AND page_id IS NOT NULL)
        OR (proposal_kind = 'create_page' AND page_id IS NULL AND proposed_path IS NOT NULL)
    );
