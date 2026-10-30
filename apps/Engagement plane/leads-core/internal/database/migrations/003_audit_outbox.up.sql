CREATE TABLE IF NOT EXISTS leads_audit_outbox (
  event_id TEXT PRIMARY KEY,
  subject TEXT NOT NULL,
  payload JSONB NOT NULL,
  attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
  next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  processing_at TIMESTAMPTZ,
  published_at TIMESTAMPTZ,
  terminal_at TIMESTAMPTZ,
  last_error TEXT,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  CHECK (length(event_id) BETWEEN 1 AND 128),
  CHECK (subject LIKE 'verevon.audit.v2.application.leads-core.%')
);

CREATE INDEX IF NOT EXISTS leads_audit_outbox_pending_idx
  ON leads_audit_outbox (next_attempt_at, created_at)
  WHERE published_at IS NULL AND terminal_at IS NULL;
