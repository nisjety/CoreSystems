-- Bounded, per-org semantic support-recurrence corpus. Deliberately owned
-- here (Application Plane), not in Data Plane's document/chunk tables: this
-- stores only a short, non-customer-facing embedding derived from a
-- ticket's category/intent/work_type/title, never a customer transcript,
-- and never becomes visible to Data Plane's shared document index.
-- Never populated for a ZDR-enabled org (enforced at build time, not just
-- purged reactively) and evicted immediately on a reactive ZDR-enabled event.
CREATE TABLE IF NOT EXISTS conversation_support_recurrence_corpus (
    org_id TEXT NOT NULL,
    ticket_id TEXT NOT NULL REFERENCES conversation_tickets(id) ON DELETE CASCADE,
    embedding REAL[] NOT NULL,
    algorithm_version TEXT NOT NULL,
    corpus_window_start TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (org_id, ticket_id),
    CHECK (char_length(algorithm_version) BETWEEN 1 AND 64),
    CHECK (array_length(embedding, 1) BETWEEN 1 AND 4096)
);

CREATE INDEX IF NOT EXISTS conversation_support_recurrence_corpus_window_idx
    ON conversation_support_recurrence_corpus (org_id, corpus_window_start);
