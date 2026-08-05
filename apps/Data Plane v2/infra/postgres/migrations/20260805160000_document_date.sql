-- P2-3: recency signal for retrieval decay. `document_date` is the SOURCE
-- content's own last-modified time (e.g. SharePoint's lastModifiedDateTime),
-- distinct from `updated_at` (when this row was last touched) and from
-- `source_objects.modified_at` (the same fact, but scoped to file-tracking
-- rows, not the content-bearing `documents` row retrieval actually reads).
--
-- Nullable, no default: most existing rows have no connector-supplied date,
-- and "unknown" must stay distinguishable from "known to be old" — the P2-3
-- decay stage treats a NULL date as no penalty, not maximum penalty.
ALTER TABLE documents ADD COLUMN IF NOT EXISTS document_date TIMESTAMPTZ;

-- One-time backfill from source_objects, which already carries this same fact
-- per file (threaded through finspo-core's separate /v1/source-objects/
-- tracking calls since before this column existed). Scoped to
-- type='sharepoint_file': that is the only document type whose `metadata`
-- carries the (drive_id, item_id) pair source_objects is keyed on.
-- sharepoint_file's own metadata is set in
-- finspo-core/internal/content/ingestor.go; sharepoint_page documents carry
-- no drive_id/item_id and are NOT covered by this backfill — they gain a
-- document_date only once ingested again through the now-fixed contract.
-- Idempotent: re-running matches zero rows once applied.
UPDATE documents d
SET document_date = so.modified_at
FROM source_objects so
WHERE d.document_date IS NULL
  AND d.type = 'sharepoint_file'
  AND d.org_id = so.org_id
  AND d.metadata->>'drive_id' = so.drive_id
  AND d.metadata->>'item_id' = so.item_id
  AND so.modified_at IS NOT NULL;
