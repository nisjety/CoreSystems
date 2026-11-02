-- WS06-S4: what the chain verifier remembers between runs.
--
-- Additive: one new table, nothing on audit_events changes. The verifier loop
-- reads and writes it; applying it with the loop off is inert. The revert is
-- migrations/revert/009_audit_chain_verification_state.down.sql.

-- One row per org: the last verdict, how far the chain is verified (the cursor
-- the next incremental run resumes from: that row must still hash to
-- verified_through_hash) and the progress of a full run that is spread over
-- several passes. last_reason is a fixed-vocabulary token, never row content.
-- This is the verifier's own bookkeeping, not evidence: the verdict is always
-- recomputed from audit_events and the signed anchors.
CREATE TABLE IF NOT EXISTS audit_chain_verifications (
  org_id                TEXT         PRIMARY KEY,
  last_status           TEXT         NOT NULL
    CHECK (last_status IN ('verified', 'verified_with_legacy', 'broken', 'partial', 'key_unavailable')),
  last_reason           TEXT         NOT NULL DEFAULT '',
  first_broken_seq      BIGINT,
  verified_through_seq  BIGINT       NOT NULL DEFAULT 0 CHECK (verified_through_seq >= 0),
  verified_through_hash BYTEA,
  legacy_unchained_rows BIGINT       NOT NULL DEFAULT 0 CHECK (legacy_unchained_rows >= 0),
  last_run_at           TIMESTAMPTZ  NOT NULL,
  last_full_at          TIMESTAMPTZ,
  full_cursor_seq       BIGINT       NOT NULL DEFAULT 0 CHECK (full_cursor_seq >= 0),
  full_cursor_hash      BYTEA
);
