-- session-core initial schema
-- Append-only event log + thread/run/checkpoint/memory metadata.

CREATE TABLE IF NOT EXISTS threads (
    id          TEXT PRIMARY KEY,
    session_key TEXT NOT NULL,
    org_id      TEXT NOT NULL,
    user_id     TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_threads_session_key ON threads (session_key);

CREATE TABLE IF NOT EXISTS messages (
    id         TEXT PRIMARY KEY,
    thread_id  TEXT NOT NULL REFERENCES threads(id),
    role       TEXT NOT NULL,
    content    TEXT NOT NULL,
    sequence   BIGINT GENERATED ALWAYS AS IDENTITY,
    metadata   JSONB DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_messages_thread_id ON messages (thread_id, sequence);

CREATE TABLE IF NOT EXISTS runs (
    id            TEXT PRIMARY KEY,
    thread_id     TEXT NOT NULL REFERENCES threads(id),
    parent_run_id TEXT,
    agent_id      TEXT NOT NULL DEFAULT 'general-v1',
    goal          TEXT NOT NULL,
    mode          TEXT NOT NULL DEFAULT 'execute',
    status        TEXT NOT NULL DEFAULT 'queued',
    org_id        TEXT NOT NULL,
    user_id       TEXT NOT NULL,
    final_output  TEXT,
    error         TEXT,
    metadata      JSONB DEFAULT '{}',
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_runs_thread_id ON runs (thread_id, created_at);
CREATE INDEX idx_runs_status    ON runs (status) WHERE status IN ('queued', 'running');

CREATE TABLE IF NOT EXISTS checkpoints (
    id         TEXT PRIMARY KEY,
    run_id     TEXT NOT NULL REFERENCES runs(id),
    state      BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_checkpoints_run_id ON checkpoints (run_id, created_at);

CREATE TABLE IF NOT EXISTS memory_index (
    id         TEXT PRIMARY KEY,
    thread_id  TEXT NOT NULL REFERENCES threads(id),
    topic      TEXT NOT NULL,
    content    TEXT NOT NULL,
    org_id     TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_memory_thread_topic ON memory_index (thread_id, topic);

-- Append-only event log for replay and audit.
CREATE TABLE IF NOT EXISTS events (
    id         TEXT PRIMARY KEY,
    event_type TEXT NOT NULL,
    run_id     TEXT NOT NULL,
    payload    JSONB NOT NULL DEFAULT '{}',
    ts         TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_events_run_id ON events (run_id, ts, id);
-- Idempotency: prevent duplicate events.
CREATE UNIQUE INDEX idx_events_idempotency ON events (id);
