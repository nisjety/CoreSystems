-- WS14-S3: one live room file per content hash per Space.
--
-- A room file is a documents row with type = 'space_upload' and a space_ref
-- (written only from a verified Control Space import decision). The hash is
-- the sha256 imports-core records in metadata for the uploaded file's bytes.
-- documents-api checks it first and answers a typed 409 room_file_duplicate
-- that names the existing document; this index is the backstop for a race.
--
-- Scoped to type = 'space_upload', live rows with a Space, so no other
-- document type and no org-wide document is affected. A deleted room file
-- releases its hash, so the same file can be uploaded again.
CREATE UNIQUE INDEX IF NOT EXISTS uq_documents_space_file_sha256
  ON documents (org_id, space_ref, (metadata->>'sha256'))
  WHERE type = 'space_upload' AND deleted_at IS NULL AND space_ref IS NOT NULL;
