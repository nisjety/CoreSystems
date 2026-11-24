-- Reverses 0138_space_approval_phase_lapses.sql.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads
-- only the top level of migrations/. Run it by hand, against the database only, inside
-- one transaction, with session-core stopped or on a build without the lapse timer (its
-- calls would fail until it is gone). Safe to run twice. After it, the 0137 trigger body is
-- back and an approval that lapses by expiry emits no run.state_changed row again.

BEGIN;

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
            -- Only the decision that ends the wait moves it back.
            IF OLD.status <> 'requested' OR NEW.status = 'requested'
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

DROP FUNCTION IF EXISTS space_event_emit_lapsed_approval_phases();
DROP INDEX IF EXISTS approvals_requested_expiry_idx;
DROP TABLE IF EXISTS space_approval_phase_lapses;

COMMIT;
