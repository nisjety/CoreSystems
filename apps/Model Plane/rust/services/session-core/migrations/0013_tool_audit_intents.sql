CREATE TABLE IF NOT EXISTS session_tool_audit_intents (
    run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    action_id TEXT NOT NULL,
    request_id TEXT NOT NULL,
    tool TEXT NOT NULL,
    data_category TEXT NOT NULL,
    zdr BOOLEAN NOT NULL,
    status TEXT NOT NULL DEFAULT 'reserved'
        CHECK (status IN ('reserved', 'completed', 'failed')),
    reserved_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    finalized_at TIMESTAMPTZ,
    PRIMARY KEY (run_id, action_id),
    UNIQUE (run_id, action_id),
    CHECK (length(action_id) BETWEEN 1 AND 128),
    CHECK (length(request_id) BETWEEN 1 AND 128),
    CHECK (length(tool) BETWEEN 1 AND 96),
    CHECK (length(data_category) BETWEEN 1 AND 64)
);

CREATE INDEX IF NOT EXISTS session_tool_audit_intents_pending_idx
    ON session_tool_audit_intents (reserved_at)
    WHERE status = 'reserved';
