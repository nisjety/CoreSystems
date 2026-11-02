-- Reverse 20261102062000_documents_space_file_dedupe.sql.
-- Only the index goes; the room-file rows stay.
DROP INDEX IF EXISTS uq_documents_space_file_sha256;
