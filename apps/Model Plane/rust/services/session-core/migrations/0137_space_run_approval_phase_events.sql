-- The approval phase reaches the Space event log (DEPTH-WS10-S1-1, C-PRIO-06).
--
-- run_phase_sql derives `awaiting_approval` for a live run (stored `queued`, its managed
-- terminalization obligation open and inside its deadline) that holds a requested, unexpired
-- approval. Nothing writes that word to runs.status, so the runs triggers of migration 0072 never
-- emitted it: the room card, the composer block, Activity approvals and approval_needed only saw
-- it on a re-read. This migration emits the same derivation as `run.state_changed` when it moves:
--
-- * an approval is requested for a live Space run that held none: state `awaiting_approval`,
--   prev `running`;
-- * the last requested approval of a live Space run is decided (status leaves `requested`):
--   state `running`, prev `awaiting_approval`.
--
-- Ids and closed words only, through space_event_emit (its allowlist, its retention gate: a
-- zero-retention run writes nothing). A run that is not live (terminal, expired obligation)
-- emits nothing: its stored status already says where it stands. An approval that lapses by
-- expiry without a decision writes no status, so it emits nothing; readers still derive the
-- phase. Fail-open like every 0072 trigger: an error is swallowed into space_event_errors under
-- the source word `approvals_phase` and never fails the approval write.

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

DROP TRIGGER IF EXISTS space_event_approvals_phase_ins ON approvals;
CREATE CONSTRAINT TRIGGER space_event_approvals_phase_ins
    AFTER INSERT ON approvals
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW
    WHEN (NEW.status = 'requested')
    EXECUTE FUNCTION space_event_from_approvals_phase();

DROP TRIGGER IF EXISTS space_event_approvals_phase_upd ON approvals;
CREATE CONSTRAINT TRIGGER space_event_approvals_phase_upd
    AFTER UPDATE OF status ON approvals
    DEFERRABLE INITIALLY DEFERRED
    FOR EACH ROW
    WHEN (OLD.status = 'requested' AND NEW.status IS DISTINCT FROM 'requested')
    EXECUTE FUNCTION space_event_from_approvals_phase();
