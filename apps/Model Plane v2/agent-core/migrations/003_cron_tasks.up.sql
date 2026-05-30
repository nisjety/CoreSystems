-- agent-core v2: cron scheduler (CC-style ScheduleCronTool)

CREATE TABLE agent_cron_tasks (
    id              TEXT PRIMARY KEY DEFAULT uuid_generate_v4()::text,
    org_id          TEXT        NOT NULL,
    session_id      TEXT        NOT NULL,
    name            TEXT        NOT NULL,
    cron_expr       TEXT        NOT NULL,
    goal            TEXT        NOT NULL,
    policy          JSONB       NOT NULL DEFAULT '{}',
    enabled         BOOLEAN     NOT NULL DEFAULT true,
    last_run_at     TIMESTAMPTZ,
    next_run_at     TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_cron_org       ON agent_cron_tasks(org_id);
CREATE INDEX idx_cron_enabled   ON agent_cron_tasks(enabled) WHERE enabled = true;
CREATE INDEX idx_cron_next_run  ON agent_cron_tasks(next_run_at) WHERE enabled = true;
