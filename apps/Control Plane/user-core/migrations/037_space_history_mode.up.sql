-- 037_space_history_mode: opt-in history window for a room (WS04-S11).
--
--  * registered_spaces.history_mode is 'full' (default: a member reads the
--    whole transcript) or 'since_join' (a member reads only what was posted
--    from the recipient-audience revision at which they last joined).
--  * The floor itself is NOT stored: Control derives it per read decision from
--    the recipient-audience snapshots (first revision after the latest one that
--    did not contain the subject), and signs it only while
--    SPACE_HISTORY_WINDOW_CLAIM is on. The column is a presentation rule, never
--    an authorization input.
--
-- Idempotent: every statement tolerates a re-run.

BEGIN;

ALTER TABLE registered_spaces
    ADD COLUMN IF NOT EXISTS history_mode TEXT NOT NULL DEFAULT 'full';

ALTER TABLE registered_spaces DROP CONSTRAINT IF EXISTS registered_spaces_history_mode_chk;
ALTER TABLE registered_spaces
    ADD CONSTRAINT registered_spaces_history_mode_chk
    CHECK (history_mode IN ('full', 'since_join'));

COMMIT;
