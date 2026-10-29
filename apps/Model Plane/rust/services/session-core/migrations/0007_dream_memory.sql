-- Dreaming Core durable memory ledger.
--
-- `agent_memory` remains the canonical memory index; this migration adds the
-- missing session-scoped upsert key plus a small run ledger for background or
-- on-demand memory consolidation cycles.

CREATE UNIQUE INDEX IF NOT EXISTS agent_memory_org_session_scope_key_uq
    ON agent_memory (org_id, session_id, scope, key)
    WHERE session_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS dream_runs (
    id              TEXT PRIMARY KEY,
    org_id          TEXT NOT NULL,
    thread_id       TEXT,
    run_id          TEXT,
    trigger         TEXT NOT NULL DEFAULT 'append_message',
    status          TEXT NOT NULL DEFAULT 'completed',
    memories_found  INTEGER NOT NULL DEFAULT 0,
    memories_saved  INTEGER NOT NULL DEFAULT 0,
    error           TEXT NOT NULL DEFAULT '',
    started_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at    TIMESTAMPTZ,
    metadata        JSONB NOT NULL DEFAULT '{}'
);

CREATE INDEX IF NOT EXISTS dream_runs_org_started_idx
    ON dream_runs (org_id, started_at DESC);

CREATE INDEX IF NOT EXISTS dream_runs_thread_started_idx
    ON dream_runs (thread_id, started_at DESC)
    WHERE thread_id IS NOT NULL;
