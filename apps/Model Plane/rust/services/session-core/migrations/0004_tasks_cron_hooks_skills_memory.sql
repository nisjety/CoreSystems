-- 0004_tasks_cron_hooks_skills_memory.sql
--
-- Adds durable tables for:
--   - tasks, task_events, task_assignments, task_dependencies, task_artifacts
--   - cron_schedules, cron_fires
--   - hook_configs (tool-call hooks with approve/block/modify semantics)
--   - agent_skills  (per-org skill definitions injected at run time)
--   - agent_memory  (per-org/session persistent memory entries)
--
-- Conventions from 0001/0002/0003: TEXT PKs, JSONB defaults, TIMESTAMPTZ,
-- idempotent DDL, soft-delete via deleted_at.

-- ---------------------------------------------------------------------------
-- tasks
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS tasks (
    id              TEXT PRIMARY KEY,
    org_id          TEXT NOT NULL DEFAULT '',
    run_id          TEXT REFERENCES runs(id),
    parent_task_id  TEXT REFERENCES tasks(id),
    kind            TEXT NOT NULL DEFAULT 'agent',    -- agent | shell | workflow | cron | manual
    title           TEXT NOT NULL DEFAULT '',
    description     TEXT NOT NULL DEFAULT '',
    assignee        TEXT NOT NULL DEFAULT '',         -- agent_id or user_id
    status          TEXT NOT NULL DEFAULT 'created',  -- created | assigned | running | blocked | completed | failed | cancelled
    priority        INT NOT NULL DEFAULT 0,
    inputs          JSONB NOT NULL DEFAULT '[]'::jsonb,
    outputs         JSONB NOT NULL DEFAULT '[]'::jsonb,
    config_json     JSONB NOT NULL DEFAULT '{}'::jsonb,
    idempotency_key TEXT NOT NULL DEFAULT '',
    -- scheduling
    scheduled_at    TIMESTAMPTZ,
    started_at      TIMESTAMPTZ,
    completed_at    TIMESTAMPTZ,
    deadline_at     TIMESTAMPTZ,
    -- audit
    created_by      TEXT NOT NULL DEFAULT '',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS tasks_org_status_idx   ON tasks (org_id, status, created_at) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS tasks_run_idx          ON tasks (run_id) WHERE run_id IS NOT NULL AND deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS tasks_parent_idx       ON tasks (parent_task_id) WHERE parent_task_id IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS tasks_idempotency_uq
    ON tasks (org_id, idempotency_key)
    WHERE idempotency_key <> '' AND deleted_at IS NULL;

-- ---------------------------------------------------------------------------
-- task_events  (lifecycle event stream per task)
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS task_events (
    id          TEXT PRIMARY KEY,
    task_id     TEXT NOT NULL REFERENCES tasks(id),
    event_type  TEXT NOT NULL,  -- created | assigned | started | blocked | completed | failed | cancelled
    actor       TEXT NOT NULL DEFAULT '',
    payload     JSONB NOT NULL DEFAULT '{}'::jsonb,
    ts          TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS task_events_task_idx ON task_events (task_id, ts);

-- ---------------------------------------------------------------------------
-- task_assignments  (which run/agent owns a task at any point)
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS task_assignments (
    id          TEXT PRIMARY KEY,
    task_id     TEXT NOT NULL REFERENCES tasks(id),
    run_id      TEXT REFERENCES runs(id),
    assignee    TEXT NOT NULL DEFAULT '',
    assigned_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    released_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS task_assignments_task_idx ON task_assignments (task_id, assigned_at DESC);

-- ---------------------------------------------------------------------------
-- task_dependencies  (DAG edges)
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS task_dependencies (
    id              TEXT PRIMARY KEY,
    task_id         TEXT NOT NULL REFERENCES tasks(id),
    depends_on_id   TEXT NOT NULL REFERENCES tasks(id),
    kind            TEXT NOT NULL DEFAULT 'finish_to_start',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS task_deps_uq ON task_dependencies (task_id, depends_on_id);
CREATE INDEX IF NOT EXISTS task_deps_upstream_idx ON task_dependencies (depends_on_id);

-- ---------------------------------------------------------------------------
-- task_artifacts  (inputs/outputs attached to a task)
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS task_artifacts (
    id              TEXT PRIMARY KEY,
    task_id         TEXT NOT NULL REFERENCES tasks(id),
    role            TEXT NOT NULL DEFAULT 'output',   -- input | output
    kind            TEXT NOT NULL DEFAULT 'file',     -- file | text | json | image | audio | video
    name            TEXT NOT NULL DEFAULT '',
    mime_type       TEXT NOT NULL DEFAULT 'application/octet-stream',
    uri             TEXT NOT NULL DEFAULT '',         -- object-store URI
    size_bytes      BIGINT NOT NULL DEFAULT 0,
    checksum        TEXT NOT NULL DEFAULT '',
    metadata        JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS task_artifacts_task_idx ON task_artifacts (task_id, role);

-- ---------------------------------------------------------------------------
-- cron_schedules
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS cron_schedules (
    id              TEXT PRIMARY KEY,
    org_id          TEXT NOT NULL DEFAULT '',
    name            TEXT NOT NULL,
    description     TEXT NOT NULL DEFAULT '',
    schedule_expr   TEXT NOT NULL,   -- cron expression e.g. "0 9 * * 1"
    timezone        TEXT NOT NULL DEFAULT 'UTC',
    task_template   JSONB NOT NULL DEFAULT '{}'::jsonb,  -- task spec to fire
    enabled         BOOLEAN NOT NULL DEFAULT TRUE,
    last_fire_at    TIMESTAMPTZ,
    next_fire_at    TIMESTAMPTZ,
    created_by      TEXT NOT NULL DEFAULT '',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ
);

CREATE UNIQUE INDEX IF NOT EXISTS cron_schedules_org_name_uq
    ON cron_schedules (org_id, name)
    WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS cron_schedules_next_fire_idx
    ON cron_schedules (next_fire_at, enabled)
    WHERE deleted_at IS NULL;

-- ---------------------------------------------------------------------------
-- cron_fires  (history of each cron invocation)
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS cron_fires (
    id              TEXT PRIMARY KEY,
    schedule_id     TEXT NOT NULL REFERENCES cron_schedules(id),
    task_id         TEXT REFERENCES tasks(id),
    fired_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    status          TEXT NOT NULL DEFAULT 'pending',  -- pending | running | completed | failed
    error           TEXT NOT NULL DEFAULT '',
    completed_at    TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS cron_fires_schedule_idx ON cron_fires (schedule_id, fired_at DESC);

-- ---------------------------------------------------------------------------
-- hook_configs  (tool-call interceptors: approve / block / modify)
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS hook_configs (
    id                  TEXT PRIMARY KEY,
    org_id              TEXT NOT NULL,
    tool_name_pattern   TEXT NOT NULL,   -- exact name or glob like "cap.browser.*"
    hook_type           TEXT NOT NULL DEFAULT 'pre_tool',   -- pre_tool | post_tool
    action              TEXT NOT NULL DEFAULT 'approve',    -- approve | block | modify | log
    reason              TEXT NOT NULL DEFAULT '',
    modify_input        JSONB,
    modify_output       JSONB,
    priority            INT NOT NULL DEFAULT 0,
    enabled             BOOLEAN NOT NULL DEFAULT TRUE,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS hook_configs_org_idx
    ON hook_configs (org_id, enabled, priority DESC);

CREATE INDEX IF NOT EXISTS hook_configs_pattern_idx
    ON hook_configs (tool_name_pattern, org_id)
    WHERE enabled;

-- ---------------------------------------------------------------------------
-- agent_skills  (per-org skill definitions injected at run time)
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS agent_skills (
    id                      TEXT PRIMARY KEY,
    org_id                  TEXT NOT NULL,
    name                    TEXT NOT NULL,
    description             TEXT NOT NULL DEFAULT '',
    content                 TEXT NOT NULL DEFAULT '',
    trigger_keywords        JSONB NOT NULL DEFAULT '[]'::jsonb,
    trigger_file_patterns   JSONB NOT NULL DEFAULT '[]'::jsonb,
    tool_restrictions       JSONB NOT NULL DEFAULT '[]'::jsonb,
    enabled                 BOOLEAN NOT NULL DEFAULT TRUE,
    created_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS agent_skills_org_name_uq
    ON agent_skills (org_id, name);

CREATE INDEX IF NOT EXISTS agent_skills_org_idx
    ON agent_skills (org_id, enabled);

-- ---------------------------------------------------------------------------
-- agent_memory  (persistent memory entries: org-level and session-scoped)
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS agent_memory (
    id              TEXT PRIMARY KEY,
    org_id          TEXT NOT NULL,
    session_id      TEXT,           -- NULL = org-level; set = session-scoped
    scope           TEXT NOT NULL DEFAULT 'org',   -- run | thread | session | user | org | global
    key             TEXT NOT NULL,
    content         TEXT NOT NULL DEFAULT '',
    kind            TEXT NOT NULL DEFAULT 'fact',  -- fact | preference | instruction | entity_relation | artifact_summary | graph_edge | wiki_block | glossary_term | adr | policy
    confidence      DOUBLE PRECISION NOT NULL DEFAULT 1.0,
    owner           TEXT NOT NULL DEFAULT '',
    source_links    TEXT[] NOT NULL DEFAULT '{}',
    review_state    TEXT NOT NULL DEFAULT 'accepted',  -- pending | accepted | rejected | expired
    classification  TEXT NOT NULL DEFAULT '',
    expires_at      TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS agent_memory_org_scope_idx
    ON agent_memory (org_id, scope, review_state, created_at);

CREATE INDEX IF NOT EXISTS agent_memory_session_idx
    ON agent_memory (session_id)
    WHERE session_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS agent_memory_key_idx
    ON agent_memory (org_id, key);

CREATE UNIQUE INDEX IF NOT EXISTS agent_memory_org_scope_key_uq
    ON agent_memory (org_id, scope, key)
    WHERE session_id IS NULL;
