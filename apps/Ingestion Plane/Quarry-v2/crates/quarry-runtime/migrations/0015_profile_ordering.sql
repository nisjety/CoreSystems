-- WS17.P1 (plan: docs/plans/quarry-browser-build-plan-2026-10-14.md, "Slice WS17.P1").
--
-- Ordered profile saves and deletion tombstones, on the existing plaintext
-- store, before the envelope layer (WS17.E1, migration 0016) changes the
-- storage shape.
--
--   * saved_at  — the capture START time of the snapshot (clamped to now + 5 s
--                 by the store). A save whose saved_at is not newer than the
--                 row's saved_at is ignored, so an older capture never
--                 replaces a newer one. NULL (rows written before this
--                 migration, or metadata-only rows) behaves as older than
--                 anything.
--   * rev       — counts STORED saves only (an ignored save does not bump it).
--   * owner_user_id — stamped on INSERT only by the save path. Added now so
--                 WS17.P2 needs no migration; NULL means "legacy row".
--   * snapshot  — made nullable so WS17.E1 can move the payload out of the
--                 row. Existing rows keep their value.
--
-- quarry_profile_tombstones — a delete writes a tombstone in the same
-- transaction as the DELETE. A save whose saved_at is not newer than
-- deleted_at is ignored, so a zombie session cannot resurrect a deleted
-- profile. Tombstones expire 7 days after the delete (swept hourly).
--
-- Idempotent: every statement is guarded (IF NOT EXISTS / re-runnable
-- ALTER COLUMN ... DROP NOT NULL). Applied files are immutable; do not edit.

ALTER TABLE quarry_profiles
    ADD COLUMN IF NOT EXISTS saved_at       TIMESTAMPTZ NULL,
    ADD COLUMN IF NOT EXISTS rev            BIGINT      NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS owner_user_id  TEXT        NULL;

ALTER TABLE quarry_profiles
    ALTER COLUMN snapshot DROP NOT NULL;

CREATE TABLE IF NOT EXISTS quarry_profile_tombstones (
    org_id      TEXT        NOT NULL,
    profile_id  TEXT        NOT NULL,
    deleted_at  TIMESTAMPTZ NOT NULL,
    expires_at  TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (org_id, profile_id)
);

CREATE INDEX IF NOT EXISTS quarry_profile_tombstones_expiry_idx
    ON quarry_profile_tombstones (expires_at);
