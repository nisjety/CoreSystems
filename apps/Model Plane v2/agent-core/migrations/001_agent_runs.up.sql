-- agent-core v2 schema: runs, actions, todos, plans, approvals
-- Applied to: agent_core_v2_db in reasoning-postgres

CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- -------------------------------------------------------
-- schema_migrations (migration tracking)
-- -------------------------------------------------------
CREATE TABLE IF NOT EXISTS schema_migrations (
    version  INT PRIMARY KEY,
    applied  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- -------------------------------------------------------
-- agent_runs
-- -------------------------------------------------------
CREATE TABLE agent_runs (
    id                  TEXT PRIMARY KEY DEFAULT uuid_generate_v4()::text,
    session_id          TEXT        NOT NULL,
    parent_run_id       TEXT        REFERENCES agent_runs(id) ON DELETE SET NULL,
    user_id             TEXT        NOT NULL,
    org_id              TEXT,
    agent_type          TEXT        NOT NULL DEFAULT 'general',
    mode                TEXT        NOT NULL DEFAULT 'execute',
    goal                TEXT        NOT NULL,
    status              TEXT        NOT NULL DEFAULT 'queued',
    policy              JSONB       NOT NULL DEFAULT '{}',
    plan_state          JSONB,
    actions             JSONB       NOT NULL DEFAULT '[]',
    current_action_idx  INT         NOT NULL DEFAULT 0,
    checkpoint_index    INT         NOT NULL DEFAULT 0,
    checkpoint_state    JSONB,
    tool_pool_version   TEXT,
    loaded_tool_names   TEXT[]      DEFAULT '{}',
    lease_owner         TEXT,
    final_output        TEXT,
    error               TEXT,
    metadata            JSONB       NOT NULL DEFAULT '{}',
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_runs_session   ON agent_runs(session_id);
CREATE INDEX idx_runs_user      ON agent_runs(user_id);
CREATE INDEX idx_runs_status    ON agent_runs(status);
CREATE INDEX idx_runs_parent    ON agent_runs(parent_run_id) WHERE parent_run_id IS NOT NULL;
CREATE INDEX idx_runs_lease     ON agent_runs(lease_owner) WHERE lease_owner IS NOT NULL;

-- -------------------------------------------------------
-- todos
-- -------------------------------------------------------
CREATE TABLE todos (
    id              TEXT PRIMARY KEY DEFAULT uuid_generate_v4()::text,
    session_id      TEXT        NOT NULL,
    run_id          TEXT        REFERENCES agent_runs(id) ON DELETE CASCADE,
    content         TEXT        NOT NULL,
    status          TEXT        NOT NULL DEFAULT 'pending',
    owner_agent_id  TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_todos_session ON todos(session_id);
CREATE INDEX idx_todos_run     ON todos(run_id) WHERE run_id IS NOT NULL;

-- -------------------------------------------------------
-- plans
-- -------------------------------------------------------
CREATE TABLE plans (
    id          TEXT PRIMARY KEY DEFAULT uuid_generate_v4()::text,
    session_id  TEXT        NOT NULL,
    run_id      TEXT        NOT NULL REFERENCES agent_runs(id) ON DELETE CASCADE,
    status      TEXT        NOT NULL DEFAULT 'pending',
    steps       JSONB       NOT NULL DEFAULT '[]',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_plans_session ON plans(session_id);
CREATE INDEX idx_plans_run     ON plans(run_id);

-- -------------------------------------------------------
-- approvals
-- -------------------------------------------------------
CREATE TABLE approvals (
    id          TEXT PRIMARY KEY DEFAULT uuid_generate_v4()::text,
    session_id  TEXT        NOT NULL,
    run_id      TEXT        NOT NULL REFERENCES agent_runs(id) ON DELETE CASCADE,
    action_id   TEXT        NOT NULL,
    action_name TEXT        NOT NULL,
    reason      TEXT        NOT NULL DEFAULT '',
    status      TEXT        NOT NULL DEFAULT 'pending',
    decided_by  TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    decided_at  TIMESTAMPTZ
);

CREATE INDEX idx_approvals_session ON approvals(session_id);
CREATE INDEX idx_approvals_run     ON approvals(run_id);
CREATE INDEX idx_approvals_pending ON approvals(status) WHERE status = 'pending';

-- -------------------------------------------------------
-- updated_at trigger
-- -------------------------------------------------------
CREATE OR REPLACE FUNCTION set_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = now();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_runs_updated    BEFORE UPDATE ON agent_runs EXECUTE FUNCTION set_updated_at();
CREATE TRIGGER trg_todos_updated   BEFORE UPDATE ON todos      EXECUTE FUNCTION set_updated_at();
CREATE TRIGGER trg_plans_updated   BEFORE UPDATE ON plans      EXECUTE FUNCTION set_updated_at();
