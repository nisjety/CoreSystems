-- E3.5-6: monitor and source tag column (control migration 027).
--
-- A tag is one operator label ("pricing", "competitor", "eu") that groups the
-- change monitors (schedules with target_kind change_monitor) and the tracked
-- sources (quarry_sources) of one org, and lets a list be filtered by it. Each
-- row keeps its org_id, and every tag read also filters on org_id, so a tag
-- never reveals another tenant's row.
--
-- Numbering: the estimate and the 2026-10-23 decisions named 021 for this column.
-- 027 is the number on disk now: INNH-5 holds 025 and WS15.S3 holds 026.
--
-- The column is TEXT NOT NULL DEFAULT '' on purpose. '' is the untagged value, so
-- every pre-027 row reads back exactly as before, and a NULL never reaches the Go
-- scan. quarry-control normalises the value before it writes it (store.NormalizeTag:
-- trimmed, lowercased, 1 to 32 characters of a-z, 0-9, '.', '_' and '-'). The
-- column does not repeat that check.
--
-- The partial indexes serve the tenant tag list (org_id, tag, newest first) and
-- skip untagged rows. They are built without CONCURRENTLY, so each build takes a
-- lock on its table for the duration; these are small control tables.
--
-- Idempotent: every statement is IF NOT EXISTS-safe, so a re-run changes nothing.

ALTER TABLE schedules      ADD COLUMN IF NOT EXISTS tag TEXT NOT NULL DEFAULT '';
ALTER TABLE quarry_sources ADD COLUMN IF NOT EXISTS tag TEXT NOT NULL DEFAULT '';

CREATE INDEX IF NOT EXISTS schedules_org_tag_created_idx
    ON schedules (org_id, tag, created_at DESC)
    WHERE tag <> '';

CREATE INDEX IF NOT EXISTS quarry_sources_org_tag_created_idx
    ON quarry_sources (org_id, tag, created_at DESC)
    WHERE tag <> '' AND deleted_at IS NULL;
