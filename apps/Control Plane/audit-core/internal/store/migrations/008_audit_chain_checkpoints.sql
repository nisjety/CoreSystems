-- WS06-S3: signed retention checkpoints, signed head attestations and the
-- incident log that backs the chain-safe purge.
--
-- Additive: three new tables, nothing on audit_events changes. Whether any of
-- it is used is decided at runtime (AUDIT_CHAIN_PRUNE_ENABLED, the attestor
-- needing a signing key), so applying this with the switches off is inert. The
-- revert is migrations/revert/008_audit_chain_checkpoints.down.sql.

-- One row per purge batch: "rows 1..through_seq of this org's chain were
-- removed on purpose, and the row at through_seq had this hash". The signature
-- covers org, through_seq, through_hash, the cumulative pruned_rows_total and
-- created_at (chain.CheckpointPayload), so a verifier can start a chain at the
-- newest checkpoint instead of at genesis, and a deleted checkpoint shows up as
-- a chain that no longer starts where it should. created_at is written by the
-- purger at microsecond precision and stored verbatim: it is part of the
-- signed bytes, so it must not default to NOW().
CREATE TABLE IF NOT EXISTS audit_chain_checkpoints (
  id                BIGSERIAL    PRIMARY KEY,
  org_id            TEXT         NOT NULL,
  through_seq       BIGINT       NOT NULL CHECK (through_seq >= 1),
  through_hash      BYTEA        NOT NULL,
  pruned_rows_total BIGINT       NOT NULL CHECK (pruned_rows_total >= 0),
  reason            TEXT         NOT NULL CHECK (reason IN ('retention', 'erasure')),
  key_id            TEXT         NOT NULL,
  signature         BYTEA        NOT NULL,
  created_at        TIMESTAMPTZ  NOT NULL,
  UNIQUE (org_id, through_seq)
);

-- A signed statement of an org's chain head at one moment. Attestations are
-- what makes truncation of the tail detectable: the head can never be
-- legitimately lower than the newest attested sequence.
CREATE TABLE IF NOT EXISTS audit_chain_attestations (
  org_id    TEXT         NOT NULL,
  chain_seq BIGINT       NOT NULL CHECK (chain_seq >= 1),
  row_hash  BYTEA        NOT NULL,
  key_id    TEXT         NOT NULL,
  signature BYTEA        NOT NULL,
  signed_at TIMESTAMPTZ  NOT NULL,
  PRIMARY KEY (org_id, chain_seq)
);

-- Why a purge deleted nothing for an org. Written outside the purge
-- transaction so the rollback that protects the rows cannot take the evidence
-- of the refusal with it. detail is a short fixed-vocabulary reason, never row
-- content.
CREATE TABLE IF NOT EXISTS audit_chain_incidents (
  id            BIGSERIAL    PRIMARY KEY,
  org_id        TEXT         NOT NULL,
  kind          TEXT         NOT NULL,
  first_bad_seq BIGINT,
  detail        TEXT         NOT NULL DEFAULT '',
  created_at    TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_audit_chain_incidents_org_created
  ON audit_chain_incidents (org_id, created_at DESC);
