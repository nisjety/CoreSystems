-- Match facts are separate from list metadata so catalogue reads remain small.
ALTER TABLE catalogue_projection ADD COLUMN matching_payload jsonb;
ALTER TABLE catalogue_projection ADD COLUMN active_until timestamptz;
CREATE INDEX catalogue_active_until ON catalogue_projection(active_until,raw_document_id)
 WHERE matching_payload IS NOT NULL;
