-- Audience labels on agent_memory (build plan WS17-S3; design WS17 D2).
--
-- One migration holds the whole label schema (conflict C12). Sibling (b)'s 0103 already added the
-- instance and audience columns (`scope_id`, `audience_hash`, `audience_revision`, `source_label`)
-- and the closed `scope` kind set (`space` and `agent` included). So the kind stays `scope`: there
-- is no second `scope_kind` column (one vocabulary, D12). The columns 0103 added are restated here
-- with IF NOT EXISTS, which is a no-op on a branch where 0103 ran. This file adds:
--
--   origin_kind  personal | room | declassified | room_unverified (NOT NULL, default personal)
--   space_ref    the Space a room-labelled row belongs to (equal to scope_id for scope 'space')
--
-- The label check: a row that is not personal or withheld names its Space and carries the audience
-- it was said to (a hash, and a revision above zero). Rows written before this migration are
-- labelled, never guessed:
--   * scope 'space' and 'agent' rows (sibling (b)'s room memory) that carry their audience become
--     origin 'room' with space_ref taken from the scope id;
--   * scope 'space' and 'agent' rows without a usable audience cannot be verified and become
--     'room_unverified';
--   * every other row whose source_links name a thread of a shared room, or whose session is one,
--     becomes 'room_unverified': withheld from every recall, listed to its owner (WS17-S2).
--
-- No unique index changes. Sibling (b)'s writers use the arbiter (org_id, scope, owner, key) WHERE
-- session_id IS NULL with a key prefixed by the Space id (space_key), which already keeps two rooms
-- of one org apart (K4-13). Changing that arbiter would break its ON CONFLICT target.
--
-- Constraints are added NOT VALID, backfilled, then validated in this file, so a backfill that
-- breaks the check fails the migration rather than leaving unchecked rows. The backfill is one
-- statement at current volume; if agent_memory grows past about a million rows, run it in batches
-- from a startup task (UNVERIFIED row count).
-- Reversal: revert/0081_agent_memory_audience_labels.down.sql.

ALTER TABLE agent_memory
    ADD COLUMN IF NOT EXISTS scope_id          TEXT,
    ADD COLUMN IF NOT EXISTS audience_hash     TEXT,
    ADD COLUMN IF NOT EXISTS audience_revision BIGINT,
    ADD COLUMN IF NOT EXISTS source_label      TEXT,
    ADD COLUMN IF NOT EXISTS origin_kind       TEXT NOT NULL DEFAULT 'personal',
    ADD COLUMN IF NOT EXISTS space_ref         TEXT;

-- 1. Sibling room memory that carries its audience: a room row or an agent note of one Space.
UPDATE agent_memory
   SET origin_kind = 'room',
       space_ref = NULLIF(
           CASE WHEN scope = 'space' THEN scope_id ELSE split_part(scope_id, ':', 1) END, '')
 WHERE scope IN ('space', 'agent')
   AND origin_kind = 'personal'
   AND scope_id IS NOT NULL AND scope_id <> ''
   AND CASE WHEN scope = 'space' THEN scope_id ELSE split_part(scope_id, ':', 1) END <> ''
   AND audience_hash IS NOT NULL AND audience_hash <> ''
   AND audience_revision IS NOT NULL AND audience_revision > 0;

-- 2. Room memory whose audience cannot be verified: withheld, not guessed.
UPDATE agent_memory
   SET origin_kind = 'room_unverified',
       space_ref = NULLIF(
           CASE WHEN scope = 'space' THEN scope_id ELSE split_part(scope_id, ':', 1) END, '')
 WHERE scope IN ('space', 'agent')
   AND origin_kind = 'personal';

-- 3. Legacy rows learned in a shared room: a source link or the session names a contained thread.
--    Withheld everywhere (C7 made this a read rule; the label makes it a stored fact).
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
                      AND st.space_id IS NOT NULL AND st.space_kind IS DISTINCT FROM 'personal'));

ALTER TABLE agent_memory DROP CONSTRAINT IF EXISTS agent_memory_origin_kind_chk;
ALTER TABLE agent_memory ADD CONSTRAINT agent_memory_origin_kind_chk CHECK (
    origin_kind IN ('personal', 'room', 'declassified', 'room_unverified')
) NOT VALID;

ALTER TABLE agent_memory DROP CONSTRAINT IF EXISTS agent_memory_label_chk;
ALTER TABLE agent_memory ADD CONSTRAINT agent_memory_label_chk CHECK (
    origin_kind IN ('personal', 'room_unverified')
    OR (space_ref IS NOT NULL AND space_ref <> ''
        AND audience_hash IS NOT NULL AND audience_hash <> ''
        AND audience_revision IS NOT NULL AND audience_revision > 0)
) NOT VALID;

ALTER TABLE agent_memory VALIDATE CONSTRAINT agent_memory_origin_kind_chk;
ALTER TABLE agent_memory VALIDATE CONSTRAINT agent_memory_label_chk;

-- The Space-scoped index of the label predicate (room recall, the Space purge, the owner's list).
CREATE INDEX IF NOT EXISTS agent_memory_space_idx
    ON agent_memory (org_id, space_ref, scope)
    WHERE space_ref IS NOT NULL;
