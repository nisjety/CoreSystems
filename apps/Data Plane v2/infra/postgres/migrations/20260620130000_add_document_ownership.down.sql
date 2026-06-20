-- Revert PR-2 document ownership columns. The migrator wraps this file in a
-- transaction; do not declare BEGIN/COMMIT here.
DROP INDEX IF EXISTS idx_documents_owner;
ALTER TABLE documents DROP CONSTRAINT IF EXISTS documents_visibility_chk;
ALTER TABLE documents DROP COLUMN IF EXISTS visibility;
ALTER TABLE documents DROP COLUMN IF EXISTS owner_id;
