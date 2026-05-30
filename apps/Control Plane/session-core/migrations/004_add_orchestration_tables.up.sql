-- Migration 004: orchestration tables (plans, plan_steps, approvals, todos, subagent_lineage_edges)
-- Enum values are stored as TEXT with CHECK constraints to remain portable
-- and avoid the pain of altering Postgres ENUMs across environments.

-- ─── plans ───────────────────────────────────────────────────────────────────

CREATE TABLE plans (
    id          TEXT        NOT NULL PRIMARY KEY,   -- ULID, prefix 'plan_'
    run_id      TEXT        NOT NULL,
    thread_id   TEXT        NOT NULL,
    author      TEXT        NOT NULL DEFAULT '',
    state       TEXT        NOT NULL DEFAULT 'DRAFT'
                CHECK (state IN (
                    'DRAFT','PROPOSED','APPROVED','REJECTED',
                    'EXECUTING','COMPLETED','FAILED','SUPERSEDED','ARCHIVED'
                )),
    summary     TEXT        NOT NULL DEFAULT '',
    supersedes  TEXT        NOT NULL DEFAULT '',    -- id of superseded plan, empty when N/A
    metadata    JSONB       NOT NULL DEFAULT '{}',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_plans_run_id      ON plans (run_id);
CREATE INDEX idx_plans_thread_id   ON plans (thread_id);
CREATE INDEX idx_plans_state       ON plans (state);

-- ─── plan_steps ──────────────────────────────────────────────────────────────

CREATE TABLE plan_steps (
    id          TEXT        NOT NULL PRIMARY KEY,   -- ULID, prefix 'step_'
    plan_id     TEXT        NOT NULL REFERENCES plans (id) ON DELETE CASCADE,
    step_order  INTEGER     NOT NULL DEFAULT 0,     -- position within the plan (0-based)
    title       TEXT        NOT NULL DEFAULT '',
    operation   TEXT        NOT NULL DEFAULT '',    -- optional execution-op slug
    state       TEXT        NOT NULL DEFAULT 'PENDING'
                CHECK (state IN (
                    'PENDING','RUNNING','DONE','SKIPPED','FAILED'
                )),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_plan_steps_plan_id ON plan_steps (plan_id);
CREATE INDEX idx_plan_steps_state   ON plan_steps (state);

-- ─── approvals ───────────────────────────────────────────────────────────────

CREATE TABLE approvals (
    id              TEXT        NOT NULL PRIMARY KEY,   -- ULID, prefix 'appr_'
    run_id          TEXT        NOT NULL,
    step_id         TEXT        NOT NULL DEFAULT '',    -- empty for run-level approvals
    kind            TEXT        NOT NULL DEFAULT 'PLAN'
                    CHECK (kind IN (
                        'PLAN','TOOL_CALL','PERMISSION','DESTRUCTIVE','COST'
                    )),
    state           TEXT        NOT NULL DEFAULT 'REQUESTED'
                    CHECK (state IN (
                        'REQUESTED','GRANTED','DENIED','TIMED_OUT'
                    )),
    requested_of    TEXT        NOT NULL DEFAULT '',
    decided_by      TEXT        NOT NULL DEFAULT '',    -- empty until decided
    decision_reason TEXT        NOT NULL DEFAULT '',
    context         JSONB       NOT NULL DEFAULT '{}',
    requested_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    decided_at      TIMESTAMPTZ,                        -- NULL until decided
    expires_at      TIMESTAMPTZ                         -- NULL when no deadline
);

CREATE INDEX idx_approvals_run_id  ON approvals (run_id);
CREATE INDEX idx_approvals_step_id ON approvals (step_id) WHERE step_id <> '';
CREATE INDEX idx_approvals_state   ON approvals (state);

-- ─── todos ───────────────────────────────────────────────────────────────────

CREATE TABLE todos (
    id           TEXT        NOT NULL PRIMARY KEY,   -- ULID, prefix 'todo_'
    thread_id    TEXT        NOT NULL,
    run_id       TEXT        NOT NULL DEFAULT '',    -- empty when not tied to a run
    assignee     TEXT        NOT NULL DEFAULT '',
    title        TEXT        NOT NULL DEFAULT '',
    description  TEXT        NOT NULL DEFAULT '',
    state        TEXT        NOT NULL DEFAULT 'PENDING'
                 CHECK (state IN (
                     'PENDING','IN_PROGRESS','BLOCKED','COMPLETED','CANCELLED'
                 )),
    priority     TEXT        NOT NULL DEFAULT 'NORMAL'
                 CHECK (priority IN (
                     'LOW','NORMAL','HIGH','URGENT'
                 )),
    blocked_by   JSONB       NOT NULL DEFAULT '[]',  -- JSON array of todo ids
    metadata     JSONB       NOT NULL DEFAULT '{}',
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at TIMESTAMPTZ                         -- NULL until terminal state
);

CREATE INDEX idx_todos_thread_id ON todos (thread_id);
CREATE INDEX idx_todos_run_id    ON todos (run_id) WHERE run_id <> '';
CREATE INDEX idx_todos_state     ON todos (state);
CREATE INDEX idx_todos_assignee  ON todos (assignee) WHERE assignee <> '';

-- ─── subagent_lineage_edges ───────────────────────────────────────────────────

CREATE TABLE subagent_lineage_edges (
    parent_run_id TEXT        NOT NULL,
    child_run_id  TEXT        NOT NULL,
    role          TEXT        NOT NULL DEFAULT 'GENERIC'
                  CHECK (role IN (
                      'CODER','REVIEWER','RESEARCHER','EXPLORER','GENERIC'
                  )),
    spawned_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    PRIMARY KEY (parent_run_id, child_run_id)
);

CREATE INDEX idx_lineage_parent ON subagent_lineage_edges (parent_run_id);
CREATE INDEX idx_lineage_child  ON subagent_lineage_edges (child_run_id);
