DROP INDEX IF EXISTS idx_documents_space_ref;
ALTER TABLE documents DROP CONSTRAINT IF EXISTS documents_space_ref_chk;
ALTER TABLE documents DROP COLUMN IF EXISTS space_ref;
