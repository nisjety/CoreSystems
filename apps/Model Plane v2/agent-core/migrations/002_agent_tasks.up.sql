-- agent-core v2: task system (CC-style task tracking with blocking/claiming)

CREATE TABLE agent_tasks (
    id              TEXT PRIMARY KEY DEFAULT uuid_generate_v4()::text,
    run_id          TEXT        NOT NULL REFERENCES agent_runs(id) ON DELETE CASCADE,
    session_id      TEXT        NOT NULL,
    org_id          TEXT,
    subject         TEXT        NOT NULL,
    description     TEXT        NOT NULL DEFAULT '',
    status          TEXT        NOT NULL DEFAULT 'pending',
    owner_agent_id  TEXT,
    blocks          TEXT[]      NOT NULL DEFAULT '{}',
    blocked_by      TEXT[]      NOT NULL DEFAULT '{}',
    metadata        JSONB       NOT NULL DEFAULT '{}',
    output          TEXT,
    error           TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_tasks_run       ON agent_tasks(run_id);
CREATE INDEX idx_tasks_session   ON agent_tasks(session_id);
CREATE INDEX idx_tasks_status    ON agent_tasks(status);
CREATE INDEX idx_tasks_owner     ON agent_tasks(owner_agent_id) WHERE owner_agent_id IS NOT NULL;
