-- An approval that lapses by expiry ends the wait too (C-PRIO-06, DEPTH-WS10-S1-1 residual).
--
-- Migration 0137 emits `run.state_changed` when a live Space run's first approval opens and when
-- its last one is decided. An approval that simply expires writes no status, so the stream kept
-- the run on `awaiting_approval` while run_phase_sql already read `running`. This migration closes
-- that gap without touching approvals.status (the gates and terminalization read an expired
-- `requested` row as they always did):
--
-- * space_event_emit_lapsed_approval_phases(), called by Session Core on a short timer, finds
--   requested approvals that expired within the last 15 minutes, marks each one in
--   space_approval_phase_lapses (so it is handled once), and emits `running` / prev
--   `awaiting_approval` for a live Space run that holds no other live requested approval: the
--   same live-run and unexpired-requested predicates as 0137 and run_phase_sql.
-- * The 0137 trigger function is replaced so that a later decision of an approval whose lapse was
--   already emitted emits nothing more.
--
-- Ids and closed words only, through space_event_emit (its allowlist, its retention gate: a
-- zero-retention run writes nothing). Nothing is marked or emitted while the event log is off, and
-- the 15-minute window keeps a later switch-on from replaying old lapses. Marks older than a day are
-- pruned. Fail-open under the existing source word `approvals_phase`.

CREATE TABLE IF NOT EXISTS space_approval_phase_lapses (
    approval_id TEXT PRIMARY KEY,
    run_id      TEXT NOT NULL,
    emitted_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS space_approval_phase_lapses_emitted_idx
    ON space_approval_phase_lapses (emitted_at);

CREATE INDEX IF NOT EXISTS approvals_requested_expiry_idx
    ON approvals (expires_at)
    WHERE status = 'requested' AND expires_at IS NOT NULL;

CREATE OR REPLACE FUNCTION space_event_emit_lapsed_approval_phases()
RETURNS INTEGER
LANGUAGE plpgsql
AS $$
DECLARE
    v_lapse RECORD;
    v_run   runs%ROWTYPE;
    v_count INTEGER := 0;
BEGIN
    DELETE FROM space_approval_phase_lapses WHERE emitted_at < now() - interval '1 day';
    IF NOT COALESCE((SELECT s.enabled FROM space_event_settings AS s WHERE s.singleton), FALSE) THEN
        RETURN 0;
    END IF;
    FOR v_lapse IN
        SELECT DISTINCT a.run_id, a.org_id
          FROM approvals AS a
         WHERE a.status = 'requested'
           AND a.expires_at IS NOT NULL
           AND a.expires_at <= now()
           AND a.expires_at > now() - interval '15 minutes'
           AND NOT EXISTS (
               SELECT 1 FROM space_approval_phase_lapses AS l WHERE l.approval_id = a.id
           )
    LOOP
        BEGIN
            -- Every lapsed approval of the run is marked, so the run moves at most once.
            INSERT INTO space_approval_phase_lapses (approval_id, run_id)
            SELECT a.id, a.run_id
              FROM approvals AS a
             WHERE a.run_id = v_lapse.run_id
               AND a.status = 'requested'
               AND a.expires_at IS NOT NULL
               AND a.expires_at <= now()
            ON CONFLICT (approval_id) DO NOTHING;
            SELECT * INTO v_run FROM runs WHERE id = v_lapse.run_id AND org_id = v_lapse.org_id;
            IF NOT FOUND OR v_run.space_id IS NULL OR v_run.retention_class = 'zdr'
               OR v_run.status <> 'queued' THEN
                CONTINUE;
            END IF;
            IF NOT EXISTS (
                SELECT 1 FROM managed_run_terminalization_outbox AS o
                 WHERE o.run_id = v_run.id
                   AND o.state IN ('open', 'processing')
                   AND o.deadline_at > now()
            ) THEN
                CONTINUE;
            END IF;
            IF EXISTS (
                SELECT 1 FROM approvals AS a
                 WHERE a.run_id = v_run.id
                   AND a.status = 'requested'
                   AND (a.expires_at IS NULL OR a.expires_at > now())
            ) THEN
                CONTINUE;
            END IF;
            PERFORM space_event_emit(
                v_run.org_id, v_run.space_id, 'run.state_changed', 'agent', v_run.agent_id,
                v_run.thread_id, v_run.id, NULL, v_run.recipient_audience_revision,
                v_run.retention_class,
                jsonb_strip_nulls(jsonb_build_object(
                    'state', 'running',
                    'prev', 'awaiting_approval',
                    'agent_id', v_run.agent_id,
                    'parent_run_id', v_run.parent_run_id
                ))
            );
            v_count := v_count + 1;
        EXCEPTION WHEN OTHERS THEN
            INSERT INTO space_event_errors (source, sqlstate) VALUES ('approvals_phase', SQLSTATE);
        END;
    END LOOP;
    RETURN v_count;
END
$$;

CREATE OR REPLACE FUNCTION space_event_from_approvals_phase()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    v_run   runs%ROWTYPE;
    v_state TEXT;
    v_prev  TEXT;
BEGIN
    BEGIN
        SELECT * INTO v_run FROM runs WHERE id = NEW.run_id AND org_id = NEW.org_id;
        IF NOT FOUND OR v_run.space_id IS NULL OR v_run.retention_class = 'zdr'
           OR v_run.status <> 'queued' THEN
            RETURN NULL;
        END IF;
        IF NOT EXISTS (
            SELECT 1 FROM managed_run_terminalization_outbox AS o
             WHERE o.run_id = v_run.id
               AND o.state IN ('open', 'processing')
               AND o.deadline_at > now()
        ) THEN
            RETURN NULL;
        END IF;
        IF TG_OP = 'INSERT' THEN
            -- Only the approval that starts the wait moves the phase.
            IF NEW.status <> 'requested'
               OR (NEW.expires_at IS NOT NULL AND NEW.expires_at <= now())
               OR EXISTS (
                   SELECT 1 FROM approvals AS a
                    WHERE a.run_id = v_run.id AND a.id <> NEW.id
                      AND a.status = 'requested'
                      AND (a.expires_at IS NULL OR a.expires_at > now())
               ) THEN
                RETURN NULL;
            END IF;
            v_state := 'awaiting_approval';
            v_prev := 'running';
        ELSE
            -- Only the decision that ends the wait moves it back, and not when the lapse sweep
            -- already reported this approval's expiry (0138).
            IF OLD.status <> 'requested' OR NEW.status = 'requested'
               OR EXISTS (
                   SELECT 1 FROM space_approval_phase_lapses AS l WHERE l.approval_id = OLD.id
               )
               OR EXISTS (
                   SELECT 1 FROM approvals AS a
                    WHERE a.run_id = v_run.id
                      AND a.status = 'requested'
                      AND (a.expires_at IS NULL OR a.expires_at > now())
               ) THEN
                RETURN NULL;
            END IF;
            v_state := 'running';
            v_prev := 'awaiting_approval';
        END IF;
        PERFORM space_event_emit(
            v_run.org_id, v_run.space_id, 'run.state_changed', 'agent', v_run.agent_id,
            v_run.thread_id, v_run.id, NULL, v_run.recipient_audience_revision,
            v_run.retention_class,
            jsonb_strip_nulls(jsonb_build_object(
                'state', v_state,
                'prev', v_prev,
                'agent_id', v_run.agent_id,
                'parent_run_id', v_run.parent_run_id
            ))
        );
    EXCEPTION WHEN OTHERS THEN
        INSERT INTO space_event_errors (source, sqlstate) VALUES ('approvals_phase', SQLSTATE);
    END;
    RETURN NULL;
END
$$;
