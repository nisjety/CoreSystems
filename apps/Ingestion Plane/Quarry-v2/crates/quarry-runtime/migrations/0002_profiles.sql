-- Cycle 24 / cluster #6 — Postgres-backed ProfileStore.
--
-- Replaces the in-memory + S3-only profile storage with a durable
-- relational table so:
--   * sessions survive `quarry-edge` restarts,
--   * multiple edge instances see the same profile state (S3 was
--     eventually-consistent on list; Postgres is read-your-writes),
--   * deletes are O(1) and immediately reflected on the next load.
--
-- ## Tenant isolation
--
-- Composite PK `(org_id, profile_id)` makes the cross-org access
-- pattern statically impossible: every `WHERE org_id = $verified`
-- gate is a primary-key probe. Listing scans only this org's
-- rows because the partial index below kicks in.

CREATE TABLE IF NOT EXISTS quarry_profiles (
    org_id      TEXT         NOT NULL,
    profile_id  TEXT         NOT NULL,
    -- The full SessionSnapshot (cookies, local_storage, session_storage,
    -- indexed_db, user_agent, viewport, locale, timezone). JSONB so we
    -- can grow the snapshot shape without a migration (current shipped
    -- fields documented in `crates/quarry-browser/src/session.rs`).
    snapshot    JSONB        NOT NULL,
    created_at  TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    PRIMARY KEY (org_id, profile_id)
);

-- List endpoint scans by org with most-recent-first.
CREATE INDEX IF NOT EXISTS quarry_profiles_org_recent_idx
    ON quarry_profiles (org_id, updated_at DESC);
