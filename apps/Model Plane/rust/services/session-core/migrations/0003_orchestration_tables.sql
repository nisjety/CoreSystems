-- 0003_orchestration_tables.sql
--
-- Adds orchestration shell tables for the Model Plane parity work
-- (Phase 9 in docs/VERIFICATION.md). Mirrors conventions from
-- 0001_init.sql and 0002_events_and_ordinals.sql:
--   * TEXT primary keys (caller-supplied ULID-prefixed ids: plan_, step_,
--     todo_, appr_).
--   * TIMESTAMPTZ NOT NULL DEFAULT now() for created_at / updated_at / ts.
--   * JSONB metadata DEFAULT '{}'.
--   * Idempotent CREATE TABLE IF NOT EXISTS / CREATE INDEX IF NOT EXISTS.
--   * plpgsql BEFORE INSERT trigger to assign monotonic per-parent ordinals
--     (MAX(parent.ordinal)+1). The single-writer-per-parent assumption from
--     0002 carries over: ordinals are unique per parent under a single
--     writer; concurrent inserts under the same parent require external
--     serialization or a SELECT ... FOR UPDATE on the parent.
--   * Partial unique index on (org_id, idempotency_key) WHERE key <> ''
--     for approval decisions (mirrors events idempotency pattern).
--
-- State string values track the proto enums in
-- proto/model_plane/v1/orchestration.proto (snake_case lowercased).
-- We intentionally store states as TEXT (not enum types) to match the
-- existing house style and to allow forward-compatible additions without
-- ALTER TYPE round-trips.

-- ---------------------------------------------------------------------------
-- plans
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS plans (
    id           TEXT PRIMARY KEY,
    thread_id    TEXT NOT NULL REFERENCES threads(id),
    run_id       TEXT REFERENCES runs(id),
    status       TEXT NOT NULL DEFAULT 'draft',
    goal         TEXT NOT NULL DEFAULT '',
    org_id       TEXT NOT NULL DEFAULT '',
    user_id      TEXT NOT NULL DEFAULT '',
    metadata     JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_plans_thread_id   ON plans (thread_id, created_at, id);
CREATE INDEX IF NOT EXISTS idx_plans_run_id      ON plans (run_id) WHERE run_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_plans_org_status  ON plans (org_id, status, created_at);

-- ---------------------------------------------------------------------------
-- plan_steps (ordinal assigned via trigger, MAX+1 per plan_id)
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS plan_steps (
    id           TEXT PRIMARY KEY,
    plan_id      TEXT NOT NULL REFERENCES plans(id),
    ordinal      BIGINT NOT NULL DEFAULT 0,
    kind         TEXT NOT NULL DEFAULT '',
    status       TEXT NOT NULL DEFAULT 'pending',
    payload      JSONB NOT NULL DEFAULT '{}'::jsonb,
    metadata     JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_plan_steps_plan_ordinal
    ON plan_steps (plan_id, ordinal);
CREATE INDEX IF NOT EXISTS idx_plan_steps_plan_status
    ON plan_steps (plan_id, status, ordinal);

CREATE OR REPLACE FUNCTION assign_plan_step_ordinal()
RETURNS TRIGGER AS $$
BEGIN
    IF NEW.ordinal IS NULL OR NEW.ordinal = 0 THEN
        SELECT COALESCE(MAX(ordinal), 0) + 1
          INTO NEW.ordinal
          FROM plan_steps
         WHERE plan_id = NEW.plan_id;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_assign_plan_step_ordinal ON plan_steps;
CREATE TRIGGER trg_assign_plan_step_ordinal
    BEFORE INSERT ON plan_steps
    FOR EACH ROW EXECUTE FUNCTION assign_plan_step_ordinal();

-- ---------------------------------------------------------------------------
-- todos (ordinal per plan_id; priority is plain TEXT to mirror state cols)
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS todos (
    id           TEXT PRIMARY KEY,
    plan_id      TEXT NOT NULL REFERENCES plans(id),
    thread_id    TEXT REFERENCES threads(id),
    ordinal      BIGINT NOT NULL DEFAULT 0,
    content      TEXT NOT NULL DEFAULT '',
    status       TEXT NOT NULL DEFAULT 'pending',
    priority     TEXT NOT NULL DEFAULT 'normal',
    metadata     JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_todos_plan_ordinal
    ON todos (plan_id, ordinal);
CREATE INDEX IF NOT EXISTS idx_todos_plan_status
    ON todos (plan_id, status, ordinal);
CREATE INDEX IF NOT EXISTS idx_todos_thread_status
    ON todos (thread_id, status, created_at) WHERE thread_id IS NOT NULL;

CREATE OR REPLACE FUNCTION assign_todo_ordinal()
RETURNS TRIGGER AS $$
BEGIN
    IF NEW.ordinal IS NULL OR NEW.ordinal = 0 THEN
        SELECT COALESCE(MAX(ordinal), 0) + 1
          INTO NEW.ordinal
          FROM todos
         WHERE plan_id = NEW.plan_id;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_assign_todo_ordinal ON todos;
CREATE TRIGGER trg_assign_todo_ordinal
    BEFORE INSERT ON todos
    FOR EACH ROW EXECUTE FUNCTION assign_todo_ordinal();

-- ---------------------------------------------------------------------------
-- approvals
-- Idempotency: decisions are effectful; we mirror the events envelope
-- pattern from 0002 with a partial unique index on (org_id, idempotency_key)
-- WHERE key <> ''. Empty-string default keeps the column NOT NULL while
-- allowing rows that opt out of idempotency.
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS approvals (
    id              TEXT PRIMARY KEY,
    run_id          TEXT NOT NULL REFERENCES runs(id),
    plan_id         TEXT REFERENCES plans(id),
    kind            TEXT NOT NULL DEFAULT 'plan',
    status          TEXT NOT NULL DEFAULT 'requested',
    requested_by    TEXT NOT NULL DEFAULT '',
    decided_by      TEXT NOT NULL DEFAULT '',
    decision_reason TEXT NOT NULL DEFAULT '',
    org_id          TEXT NOT NULL DEFAULT '',
    user_id         TEXT NOT NULL DEFAULT '',
    idempotency_key TEXT NOT NULL DEFAULT '',
    metadata        JSONB NOT NULL DEFAULT '{}'::jsonb,
    requested_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    decided_at      TIMESTAMPTZ,
    expires_at      TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_approvals_run_id
    ON approvals (run_id, requested_at, id);
CREATE INDEX IF NOT EXISTS idx_approvals_plan_id
    ON approvals (plan_id, requested_at) WHERE plan_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_approvals_org_status
    ON approvals (org_id, status, requested_at);
CREATE UNIQUE INDEX IF NOT EXISTS idx_approvals_idempotency
    ON approvals (org_id, idempotency_key)
    WHERE idempotency_key <> '';

-- ---------------------------------------------------------------------------
-- subagent_edges
-- Composite primary key on (parent_run_id, child_run_id). A child run can
-- have at most one parent edge; we add a partial unique index to enforce
-- that without blocking re-attachment of a different child.
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS subagent_edges (
    parent_run_id  TEXT NOT NULL REFERENCES runs(id),
    child_run_id   TEXT NOT NULL REFERENCES runs(id),
    role           TEXT NOT NULL DEFAULT 'generic',
    status         TEXT NOT NULL DEFAULT 'attached',
    metadata       JSONB NOT NULL DEFAULT '{}'::jsonb,
    attached_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    detached_at    TIMESTAMPTZ,
    PRIMARY KEY (parent_run_id, child_run_id)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_subagent_edges_child_unique
    ON subagent_edges (child_run_id);
CREATE INDEX IF NOT EXISTS idx_subagent_edges_parent
    ON subagent_edges (parent_run_id, attached_at);
