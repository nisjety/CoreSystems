-- letta-bridge durable memory store.
-- Kept in sync with internal/pgstore.schemaDDL (applied idempotently in-code).
CREATE TABLE IF NOT EXISTS letta_memory_blocks (
    org_id     TEXT        NOT NULL,
    thread_id  TEXT        NOT NULL,
    memory_id  TEXT        NOT NULL,
    topic      TEXT        NOT NULL,
    content    TEXT        NOT NULL DEFAULT '',
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (org_id, thread_id, memory_id)
);

CREATE INDEX IF NOT EXISTS idx_letta_memory_org_thread
    ON letta_memory_blocks (org_id, thread_id);

CREATE INDEX IF NOT EXISTS idx_letta_memory_org_updated
    ON letta_memory_blocks (org_id, updated_at DESC);
