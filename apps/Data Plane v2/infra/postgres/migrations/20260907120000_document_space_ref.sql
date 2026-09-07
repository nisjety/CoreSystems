-- Which Space a document was imported into.
--
-- `documents-api` already verifies a Control-signed Space import decision on
-- `POST /v1/documents` (`internal/handler/space_import.go`) and answers with
-- `X-Space-Import-Authority-Accepted: true` — and then discards the Space the
-- authority was issued for. The row that results is indistinguishable from any
-- other org document, so nothing in Data can answer "which documents belong to
-- this room". That is why there is no Space-filtered listing: not a missing
-- query, a missing edge.
--
-- This column IS that edge. It is written only from a verified import decision's
-- own `space_ref`, never from a request body: a client-supplied Space is a claim,
-- not authority, exactly as `space_retrieval_bindings` says of a client-supplied
-- workspace filter. A NULL means "not imported under Space authority" and stays
-- NULL — org-wide documents are not retroactively assigned to a room.
--
-- The Space reference is Application's canonical `SpaceRef` string. Data stores
-- it opaquely and never parses it: membership, kind, and lifecycle stay in the
-- planes that own them.
ALTER TABLE documents ADD COLUMN IF NOT EXISTS space_ref TEXT;

ALTER TABLE documents DROP CONSTRAINT IF EXISTS documents_space_ref_chk;
ALTER TABLE documents ADD CONSTRAINT documents_space_ref_chk
    CHECK (space_ref IS NULL OR char_length(btrim(space_ref)) > 0);

-- Partial: the Space listing only ever reads rows that have one, and the vast
-- majority of documents never will.
CREATE INDEX IF NOT EXISTS idx_documents_space_ref
    ON documents (org_id, space_ref, updated_at DESC)
    WHERE space_ref IS NOT NULL AND deleted_at IS NULL;
