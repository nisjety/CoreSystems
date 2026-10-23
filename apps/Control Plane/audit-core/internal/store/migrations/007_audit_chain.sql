-- WS06-S2: per-org hash chain over audit_events.
--
-- Every column is nullable and nothing is backfilled: a row written before an
-- org's chain was activated stays unchained ("legacy_unchained") and is never
-- rewritten. Rows are chained by the inserter (chain_mode 'insert') or, for a
-- row that landed unchained after activation (the chain switch was off for a
-- while), by the boot catch-up (chain_mode 'catchup'). The row hash format is
-- internal/chain (format v1).
--
-- Whether the chain is switched on is a runtime flag (AUDIT_CHAIN_ENABLED);
-- this migration only adds the storage, so it is safe with the flag off. The
-- revert is migrations/revert/007_audit_chain.down.sql.

ALTER TABLE audit_events
  ADD COLUMN IF NOT EXISTS chain_seq     BIGINT,
  ADD COLUMN IF NOT EXISTS prev_hash     BYTEA,
  ADD COLUMN IF NOT EXISTS row_hash      BYTEA,
  ADD COLUMN IF NOT EXISTS chain_version SMALLINT,
  ADD COLUMN IF NOT EXISTS chain_mode    TEXT;

ALTER TABLE audit_events
  DROP CONSTRAINT IF EXISTS audit_events_chain_mode_check;
ALTER TABLE audit_events
  ADD CONSTRAINT audit_events_chain_mode_check
  CHECK (chain_mode IS NULL OR chain_mode IN ('insert', 'catchup'));

-- Contiguity is enforced per org by this index and by the head lock. Retention
-- prunes by contiguous chain_seq, never by ingested_at (which is the
-- transaction start time and so not commit-ordered).
CREATE UNIQUE INDEX IF NOT EXISTS uq_audit_events_org_chain
  ON audit_events (org_id, chain_seq)
  WHERE chain_seq IS NOT NULL;

-- One row per org: the chain head, and the lock every chained insert takes.
-- activated_at is the first chained insert for the org; unchained rows with
-- ingested_at at or after it are the ones catch-up may chain.
CREATE TABLE IF NOT EXISTS audit_chain_heads (
  org_id       TEXT         PRIMARY KEY,
  head_seq     BIGINT       NOT NULL CHECK (head_seq >= 0),
  head_hash    BYTEA        NOT NULL,
  activated_at TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
  updated_at   TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
  state        TEXT         NOT NULL DEFAULT 'ok' CHECK (state IN ('ok', 'broken'))
);

-- Guard rail, not a security boundary (the table owner can drop it): it makes
-- an accidental or casual UPDATE/DELETE fail loudly. Allowed are
--   * the single transition that chains a row, which may set only the five
--     chain columns, once, and only on a row that has no chain_seq yet;
--   * anything inside a transaction that set audit.maintenance = 'on'
--     (retention and test cleanup use it).
CREATE OR REPLACE FUNCTION audit_events_guard() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
  IF COALESCE(current_setting('audit.maintenance', true), '') = 'on' THEN
    IF TG_OP = 'DELETE' THEN
      RETURN OLD;
    END IF;
    RETURN NEW;
  END IF;

  IF TG_OP = 'UPDATE'
     AND OLD.chain_seq IS NULL
     AND NEW.chain_seq IS NOT NULL
     AND NEW.prev_hash IS NOT NULL
     AND NEW.row_hash IS NOT NULL
     AND NEW.chain_version IS NOT NULL
     AND NEW.chain_mode IS NOT NULL
     AND (to_jsonb(NEW) - 'chain_seq' - 'prev_hash' - 'row_hash' - 'chain_version' - 'chain_mode')
       = (to_jsonb(OLD) - 'chain_seq' - 'prev_hash' - 'row_hash' - 'chain_version' - 'chain_mode')
  THEN
    RETURN NEW;
  END IF;

  RAISE EXCEPTION 'audit_events is append-only: % is not allowed', TG_OP
    USING ERRCODE = 'restrict_violation';
END;
$$;

DROP TRIGGER IF EXISTS audit_events_guard ON audit_events;
CREATE TRIGGER audit_events_guard
  BEFORE UPDATE OR DELETE ON audit_events
  FOR EACH ROW EXECUTE FUNCTION audit_events_guard();
