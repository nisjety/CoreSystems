-- 001_initial_schema.up.sql
-- Runner inventory, tasks, and artifacts tables for execution-core.

-- Runner inventory: tracks registered runners and their status
CREATE TABLE IF NOT EXISTS runner_inventory (
    runner_id       TEXT PRIMARY KEY,
    capabilities    JSONB NOT NULL DEFAULT '[]',
    max_concurrent  INT NOT NULL DEFAULT 1,
    labels          JSONB NOT NULL DEFAULT '{}',
    status          TEXT NOT NULL DEFAULT 'idle'
                    CHECK (status IN ('idle','claimed','running','completing','cancelled','dead')),
    current_task_id TEXT,
    workspace_id    TEXT,
    last_heartbeat  TIMESTAMPTZ NOT NULL DEFAULT now(),
    registered_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_runner_status ON runner_inventory (status);
CREATE INDEX IF NOT EXISTS idx_runner_heartbeat ON runner_inventory (last_heartbeat);

-- Runner tasks: individual task assignments
CREATE TABLE IF NOT EXISTS runner_tasks (
    task_id         TEXT PRIMARY KEY,
    run_id          TEXT NOT NULL,
    session_id      TEXT NOT NULL,
    workspace_id    TEXT NOT NULL,
    runner_id       TEXT,
    tool_name       TEXT NOT NULL,
    tool_input      JSONB NOT NULL DEFAULT '{}',
    status          TEXT NOT NULL DEFAULT 'idle'
                    CHECK (status IN ('idle','claimed','running','completing','cancelled')),
    priority        INT NOT NULL DEFAULT 0,
    timeout_seconds INT NOT NULL DEFAULT 300,
    output          JSONB,
    error           TEXT,
    duration_ms     INT NOT NULL DEFAULT 0,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    started_at      TIMESTAMPTZ,
    completed_at    TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_task_run_id ON runner_tasks (run_id);
CREATE INDEX IF NOT EXISTS idx_task_runner_id ON runner_tasks (runner_id);
CREATE INDEX IF NOT EXISTS idx_task_status ON runner_tasks (status);
CREATE INDEX IF NOT EXISTS idx_task_created ON runner_tasks (created_at DESC);

-- Runner artifacts: metadata for uploaded/downloaded files
CREATE TABLE IF NOT EXISTS runner_artifacts (
    artifact_id     TEXT PRIMARY KEY,
    task_id         TEXT NOT NULL REFERENCES runner_tasks (task_id) ON DELETE CASCADE,
    workspace_id    TEXT NOT NULL,
    kind            TEXT NOT NULL CHECK (kind IN ('input','output','log','checkpoint')),
    filename        TEXT NOT NULL,
    size_bytes      BIGINT NOT NULL DEFAULT 0,
    content_type    TEXT NOT NULL DEFAULT 'application/octet-stream',
    storage_key     TEXT NOT NULL DEFAULT '',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_artifact_task ON runner_artifacts (task_id);
CREATE INDEX IF NOT EXISTS idx_artifact_workspace ON runner_artifacts (workspace_id);
