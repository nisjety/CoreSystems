-- 050_space_member_history_window: per-member room history visibility (D18).
--
--  * One row per (room, member) that overrides the room's history_mode for
--    that member. No row means the room's history_mode decides, exactly as
--    WS04-S11 does today.
--  * mode is 'room' (an explicit reset to the room's history_mode, written when
--    an invite shares the past again; reads exactly as no row), 'full' (the
--    member reads the whole past, even in a since_join room), 'since_join'
--    (join-only: everything before the member's join is hidden) or 'since_date'
--    (everything on or after since_at is visible, older is hidden). Rows are
--    never deleted, so revision and audit ids only ever advance.
--  * since_at is set for since_date only and holds an absolute instant.
--  * The since_join floor itself is NOT stored: it is derived per read from the
--    recipient-audience snapshots and the membership ledger, as WS04-S11 derives
--    it. A row holds a mode and a timestamp only, no content, so it is allowed
--    under every Zero Data Retention posture.
--  * A presentation rule, never an authorization input: a row can only hide.
--  * revision advances on every change; set_by is the acting Control user.
--
-- Idempotent: every statement tolerates a re-run.

BEGIN;

CREATE TABLE IF NOT EXISTS space_member_history_windows (
    space_ref  TEXT NOT NULL REFERENCES registered_spaces(space_ref) ON DELETE CASCADE,
    subject_id TEXT NOT NULL,
    mode       TEXT NOT NULL,
    since_at   TIMESTAMPTZ,
    set_by     TEXT NOT NULL,
    revision   BIGINT NOT NULL DEFAULT 1,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (space_ref, subject_id),
    CONSTRAINT space_member_history_windows_mode_chk
        CHECK (mode IN ('room', 'full', 'since_join', 'since_date')),
    CONSTRAINT space_member_history_windows_since_chk
        CHECK ((mode = 'since_date') = (since_at IS NOT NULL)),
    CONSTRAINT space_member_history_windows_revision_chk CHECK (revision > 0)
);

COMMIT;
