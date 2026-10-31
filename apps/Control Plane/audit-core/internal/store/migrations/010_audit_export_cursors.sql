-- WS06-S8: where each SIEM destination's export has got to.
--
-- Additive: one new table, nothing on audit_events changes. Only the SIEM
-- exporter (cmd/siem-exporter) writes it; the audit-core server applies this
-- migration and the exporter waits for it rather than migrating. With the
-- exporter off the table stays empty and inert. The revert is
-- migrations/revert/010_audit_export_cursors.down.sql.

-- One row per destination (org-core's id; a destination belongs to one org).
-- cursor_seq is the highest chain_seq the destination has acknowledged with a
-- 2xx, so a restart redelivers at most the batch that was in flight and the
-- receiver de-duplicates on (org_id, chain_seq). No destination secret or URL
-- is stored: those stay sealed in org-core and are read at delivery time.
-- last_error is a fixed-vocabulary token, never a response body or URL.
CREATE TABLE IF NOT EXISTS audit_export_cursors (
  destination_id       TEXT         PRIMARY KEY CHECK (length(destination_id) BETWEEN 1 AND 128),
  org_id               TEXT         NOT NULL CHECK (btrim(org_id) <> ''),
  kind                 TEXT         NOT NULL CHECK (kind IN ('ndjson_webhook', 'splunk_hec')),
  cursor_seq           BIGINT       NOT NULL DEFAULT 0 CHECK (cursor_seq >= 0),
  delivered_total      BIGINT       NOT NULL DEFAULT 0 CHECK (delivered_total >= 0),
  consecutive_failures INTEGER      NOT NULL DEFAULT 0 CHECK (consecutive_failures >= 0),
  last_error           TEXT         NOT NULL DEFAULT '',
  last_attempt_at      TIMESTAMPTZ,
  last_success_at      TIMESTAMPTZ,
  updated_at           TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_audit_export_cursors_org ON audit_export_cursors (org_id);
