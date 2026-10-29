-- Session-core schema: sessions, session_events, approval_queue

CREATE TABLE IF NOT EXISTS sessions (
    id              TEXT PRIMARY KEY,
    tenant_id       TEXT NOT NULL,
    workspace_id    TEXT NOT NULL,
    user_id         TEXT NOT NULL,
    model_plane_version TEXT NOT NULL DEFAULT 'v1',
    status          TEXT NOT NULL DEFAULT 'active',
    plan_mode       BOOLEAN NOT NULL DEFAULT FALSE,
    metadata        JSONB,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_sessions_user_id ON sessions (user_id);
CREATE INDEX idx_sessions_tenant_id ON sessions (tenant_id);
CREATE INDEX idx_sessions_workspace_id ON sessions (workspace_id);
CREATE INDEX idx_sessions_status ON sessions (status) WHERE status = 'active';
CREATE INDEX idx_sessions_created_at ON sessions (created_at DESC);

CREATE TABLE IF NOT EXISTS session_events (
    id          TEXT PRIMARY KEY,
    session_id  TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    sequence    BIGINT NOT NULL,
    event_type  TEXT NOT NULL,
    payload     JSONB NOT NULL DEFAULT '{}',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (session_id, sequence)
);

CREATE INDEX idx_session_events_session_seq ON session_events (session_id, sequence);
CREATE INDEX idx_session_events_type ON session_events (event_type);

CREATE TABLE IF NOT EXISTS approval_queue (
    id          TEXT PRIMARY KEY,
    session_id  TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    run_id      TEXT NOT NULL,
    tool_name   TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    status      TEXT NOT NULL DEFAULT 'pending',
    decision    TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    resolved_at TIMESTAMPTZ
);

CREATE INDEX idx_approval_queue_session_id ON approval_queue (session_id);
CREATE INDEX idx_approval_queue_pending ON approval_queue (status) WHERE status = 'pending';

-- Sequence generator for session events (per-session monotonic)
CREATE OR REPLACE FUNCTION next_session_event_sequence(p_session_id TEXT)
RETURNS BIGINT AS $$
    SELECT COALESCE(MAX(sequence), 0) + 1
    FROM session_events
    WHERE session_id = p_session_id;
$$ LANGUAGE SQL;
