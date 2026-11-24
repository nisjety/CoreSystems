-- Batched, off-peak pre-run of migration 0081's label backfill (E-PRIO-02, DEPTH-WS17-S3-ARBITER).
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads only the top level of
-- migrations/. An operator runs it by hand, with psql in autocommit mode (no surrounding BEGIN), against a database
-- that has NOT yet applied 0081 and whose agent_memory is large (about a million rows or more). It is never needed
-- on a database that already applied 0081, and never on a small one: there 0081's own single-statement backfill is
-- fine.
--
-- Why: 0081 labels every pre-existing row in three UPDATE statements inside the migration transaction, which holds
-- row locks on every relabelled row until session-core's startup migration commits. This script does the same three
-- relabels first, in batches of :batch rows, each batch its own transaction, so the later migration's UPDATEs find
-- nothing left to change (each of them only touches rows still labelled 'personal', so they are idempotent and the
-- result is the same). The constraints, their validation and the index stay in 0081 itself.
--
-- The rules are 0081's, copied exactly (labels are never guessed):
--   1. scope 'space'/'agent' rows that carry their audience -> 'room', space_ref from the scope id;
--   2. scope 'space'/'agent' rows without a usable audience -> 'room_unverified';
--   3. other rows whose source links or session name a shared-room thread -> 'room_unverified'.
--
-- Usage (an off-peak window; session-core may keep running, since every batch is short):
--   psql "$SESSION_CORE_DATABASE_URL" -v batch=5000 \
--        -f migrations/runbook/0081_agent_memory_audience_labels.batched.sql
-- Then deploy the session-core build that applies 0081. Safe to run twice; a re-run changes nothing.
-- The script prints only row counts, never content.

\set ON_ERROR_STOP on
\if :{?batch}
\else
  \set batch 5000
\endif

-- The columns 0081 adds (each IF NOT EXISTS; adding a column with a constant default is metadata-only).
ALTER TABLE agent_memory
    ADD COLUMN IF NOT EXISTS scope_id          TEXT,
    ADD COLUMN IF NOT EXISTS audience_hash     TEXT,
    ADD COLUMN IF NOT EXISTS audience_revision BIGINT,
    ADD COLUMN IF NOT EXISTS source_label      TEXT,
    ADD COLUMN IF NOT EXISTS origin_kind       TEXT NOT NULL DEFAULT 'personal',
    ADD COLUMN IF NOT EXISTS space_ref         TEXT;

-- One procedure per run, in this session's temporary schema, so nothing is left behind. COMMIT inside a procedure
-- needs CALL outside a transaction block, which psql autocommit gives.
CREATE OR REPLACE PROCEDURE pg_temp.agent_memory_label_backfill(batch_size INT)
LANGUAGE plpgsql
AS $$
DECLARE
    changed BIGINT;
    total_room BIGINT := 0;
    total_room_unverified BIGINT := 0;
    total_legacy BIGINT := 0;
BEGIN
    -- 1. Sibling room memory that carries its audience.
    LOOP
        WITH batch AS (
            SELECT id FROM agent_memory
             WHERE scope IN ('space', 'agent')
               AND origin_kind = 'personal'
               AND scope_id IS NOT NULL AND scope_id <> ''
               AND CASE WHEN scope = 'space' THEN scope_id ELSE split_part(scope_id, ':', 1) END <> ''
               AND audience_hash IS NOT NULL AND audience_hash <> ''
               AND audience_revision IS NOT NULL AND audience_revision > 0
             LIMIT batch_size
             FOR UPDATE SKIP LOCKED
        )
        UPDATE agent_memory m
           SET origin_kind = 'room',
               space_ref = NULLIF(
                   CASE WHEN m.scope = 'space' THEN m.scope_id ELSE split_part(m.scope_id, ':', 1) END, '')
          FROM batch
         WHERE m.id = batch.id;
        GET DIAGNOSTICS changed = ROW_COUNT;
        COMMIT;
        total_room := total_room + changed;
        EXIT WHEN changed = 0;
    END LOOP;

    -- 2. Room memory whose audience cannot be verified.
    LOOP
        WITH batch AS (
            SELECT id FROM agent_memory
             WHERE scope IN ('space', 'agent')
               AND origin_kind = 'personal'
             LIMIT batch_size
             FOR UPDATE SKIP LOCKED
        )
        UPDATE agent_memory m
           SET origin_kind = 'room_unverified',
               space_ref = NULLIF(
                   CASE WHEN m.scope = 'space' THEN m.scope_id ELSE split_part(m.scope_id, ':', 1) END, '')
          FROM batch
         WHERE m.id = batch.id;
        GET DIAGNOSTICS changed = ROW_COUNT;
        COMMIT;
        total_room_unverified := total_room_unverified + changed;
        EXIT WHEN changed = 0;
    END LOOP;

    -- 3. Legacy rows learned in a shared room.
    LOOP
        WITH batch AS (
            SELECT m.id FROM agent_memory m
             WHERE m.origin_kind = 'personal'
               AND m.scope NOT IN ('space', 'agent')
               AND (EXISTS (SELECT 1
                              FROM unnest(m.source_links) AS sl(link)
                              JOIN threads ct ON ct.id = substr(sl.link, 8)
                             WHERE sl.link LIKE 'thread:%'
                               AND ct.space_id IS NOT NULL AND ct.space_kind IS DISTINCT FROM 'personal')
                    OR EXISTS (SELECT 1
                                 FROM threads st
                                WHERE st.id = m.session_id
                                  AND st.space_id IS NOT NULL AND st.space_kind IS DISTINCT FROM 'personal'))
             LIMIT batch_size
             FOR UPDATE SKIP LOCKED
        )
        UPDATE agent_memory AS m
           SET origin_kind = 'room_unverified',
               space_ref = COALESCE(
                   (SELECT ct.space_id
                      FROM unnest(m.source_links) AS sl(link)
                      JOIN threads ct ON ct.id = substr(sl.link, 8)
                     WHERE sl.link LIKE 'thread:%'
                       AND ct.space_id IS NOT NULL AND ct.space_kind IS DISTINCT FROM 'personal'
                     ORDER BY ct.space_id
                     LIMIT 1),
                   (SELECT st.space_id
                      FROM threads st
                     WHERE st.id = m.session_id
                       AND st.space_id IS NOT NULL AND st.space_kind IS DISTINCT FROM 'personal'))
          FROM batch
         WHERE m.id = batch.id;
        GET DIAGNOSTICS changed = ROW_COUNT;
        COMMIT;
        total_legacy := total_legacy + changed;
        EXIT WHEN changed = 0;
    END LOOP;

    RAISE NOTICE 'agent_memory label backfill: room=%, room_unverified=%, legacy_room_unverified=%',
        total_room, total_room_unverified, total_legacy;
END;
$$;

CALL pg_temp.agent_memory_label_backfill(:batch);
