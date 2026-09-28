CREATE TABLE IF NOT EXISTS learning_attempts (
  id text PRIMARY KEY DEFAULT gen_random_uuid()::text,
  org_id text NOT NULL CHECK (btrim(org_id) <> ''),
  class_id text NOT NULL,
  assignment_id text NOT NULL,
  learner_id text NOT NULL CHECK (btrim(learner_id) <> ''),
  mode text NOT NULL CHECK (mode IN ('guide', 'demonstrate', 'practice')),
  status text NOT NULL DEFAULT 'in_progress'
    CHECK (status IN ('in_progress', 'finished', 'abandoned')),
  idempotency_hash text NOT NULL CHECK (idempotency_hash ~ '^[a-f0-9]{64}$'),
  request_hash text NOT NULL CHECK (request_hash ~ '^[a-f0-9]{64}$'),
  started_at timestamptz NOT NULL DEFAULT now(),
  finished_at timestamptz,
  UNIQUE (id, org_id, class_id),
  UNIQUE (org_id, learner_id, idempotency_hash),
  FOREIGN KEY (assignment_id, org_id, class_id)
    REFERENCES learning_assignments(id, org_id, class_id),
  CHECK ((status = 'in_progress' AND finished_at IS NULL) OR
         (status <> 'in_progress' AND finished_at IS NOT NULL))
);

CREATE INDEX IF NOT EXISTS learning_attempts_by_assignment
  ON learning_attempts(org_id, class_id, assignment_id, started_at DESC);

CREATE TABLE IF NOT EXISTS learning_attempt_events (
  id text PRIMARY KEY DEFAULT gen_random_uuid()::text,
  org_id text NOT NULL CHECK (btrim(org_id) <> ''),
  class_id text NOT NULL,
  attempt_id text NOT NULL,
  sequence integer NOT NULL CHECK (sequence > 0),
  actor_id text NOT NULL CHECK (btrim(actor_id) <> ''),
  event_type text NOT NULL CHECK (event_type IN (
    'step_started', 'assistance_used', 'learner_reported_complete',
    'blocked', 'attempt_finished', 'attempt_abandoned'
  )),
  step_id text,
  idempotency_hash text NOT NULL CHECK (idempotency_hash ~ '^[a-f0-9]{64}$'),
  request_hash text NOT NULL CHECK (request_hash ~ '^[a-f0-9]{64}$'),
  occurred_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (attempt_id, org_id, sequence),
  UNIQUE (attempt_id, org_id, idempotency_hash),
  FOREIGN KEY (attempt_id, org_id, class_id)
    REFERENCES learning_attempts(id, org_id, class_id),
  CHECK (
    (event_type IN ('attempt_finished', 'attempt_abandoned') AND step_id IS NULL) OR
    (event_type NOT IN ('attempt_finished', 'attempt_abandoned') AND step_id IS NOT NULL AND btrim(step_id) <> '')
  )
);

CREATE INDEX IF NOT EXISTS learning_attempt_events_by_attempt
  ON learning_attempt_events(org_id, attempt_id, sequence);

DO $$
DECLARE table_name text;
BEGIN
  FOREACH table_name IN ARRAY ARRAY['learning_attempts', 'learning_attempt_events'] LOOP
    EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY', table_name);
    EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY', table_name);
    EXECUTE format('DROP POLICY IF EXISTS learning_tenant_scope ON %I', table_name);
    EXECUTE format(
      'CREATE POLICY learning_tenant_scope ON %I USING (org_id = current_setting(''app.current_org_id'', true)) WITH CHECK (org_id = current_setting(''app.current_org_id'', true))',
      table_name
    );
  END LOOP;
END
$$;

REVOKE UPDATE ON learning_attempts, learning_attempt_events FROM learning_app;
GRANT SELECT, INSERT ON learning_attempts, learning_attempt_events TO learning_app;
GRANT UPDATE (status, finished_at) ON learning_attempts TO learning_app;
ALTER DEFAULT PRIVILEGES FOR ROLE learning_owner IN SCHEMA public
  GRANT SELECT, INSERT, UPDATE ON TABLES TO learning_app;
