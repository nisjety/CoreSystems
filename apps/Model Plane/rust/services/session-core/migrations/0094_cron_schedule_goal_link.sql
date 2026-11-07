-- 0094: goal-bound schedules (Wave 3 EP-7 S6; number assigned by integration round B).
--
-- "Keep working while I'm away" turns a thread goal into a WS20 schedule: each fire is an ordinary
-- scheduled run under a read-only stored-consent contract, in the service-owned scheduled thread,
-- with a fresh Control fire decision (owner decisions D6 (b) and D9, taken by their
-- recommendation). Nothing here runs in the user's identity. This migration links the schedule to
-- its goal, marks the goal turns those fires spend, and keeps the two in step:
--
-- * `cron_schedules.goal_id` is bound at insert from the template's `goal_id`, and only when the
--   goal is open-and-active, in the schedule's org, owned by the schedule's Control-verified
--   creator, on a personal (non-Space) thread that holds no waived run. A template that names a
--   goal it may not bind is refused, so a schedule never runs a goal's text unbound. The template
--   is digest-bound by Control, so the link can never be re-pointed afterwards (the guard below).
-- * A goal that leaves `active` (paused, met, blocked, a cap, cleared) pauses its schedules in the
--   same transaction (`goal_paused_reason`), whichever code path moved it. A paused goal-bound
--   schedule is re-enabled only while its goal is active again, and never after it ran the runs
--   its template allows (`goal_runs_reached`, written by session-core with the scheduled result).
-- * A goal that is deleted (its thread or its org erased) sets the link NULL (ON DELETE SET NULL)
--   and disables the schedule, so it never outlives its goal as an ordinary schedule.
-- * `thread_goal_turns.unattended` / `scheduled_thread_id` mark the turns a fire spent; the row is
--   written with the scheduled result (session-core `RecordScheduledRunResult`).
--
-- Zero Data Retention: no content. Ids, a closed reason token and a flag. The scheduled lane
-- refuses ZDR and waived runs (`DeniesZDR`, DEC-32), and a goal never exists on such a thread
-- (0091). Erasure: the link and the turns go with their goal; the schedule goes with its org.
--
-- Additive only: two nullable columns on cron_schedules (no default, no rewrite), two columns with
-- constant defaults on thread_goal_turns (metadata-only), one partial index on a column that is
-- NULL on every existing row, and three triggers that act only on goal-bound rows.

ALTER TABLE cron_schedules
    ADD COLUMN IF NOT EXISTS goal_id TEXT NULL REFERENCES thread_goals(id) ON DELETE SET NULL;
-- Why a goal-bound schedule was paused by its goal: a closed token, NULL while it runs.
ALTER TABLE cron_schedules
    ADD COLUMN IF NOT EXISTS goal_paused_reason TEXT NULL
        CHECK (goal_paused_reason IS NULL OR goal_paused_reason ~ '^[a-z0-9_]{1,64}$');

-- One driver per goal: at most one live schedule is bound to a goal.
CREATE UNIQUE INDEX IF NOT EXISTS cron_schedules_goal_idx
    ON cron_schedules (goal_id)
    WHERE goal_id IS NOT NULL AND deleted_at IS NULL;

ALTER TABLE thread_goal_turns
    ADD COLUMN IF NOT EXISTS unattended BOOLEAN NOT NULL DEFAULT false;
-- The service-owned thread an unattended turn's answer went to (D9); NULL for an attended turn.
ALTER TABLE thread_goal_turns
    ADD COLUMN IF NOT EXISTS scheduled_thread_id TEXT NULL;

-- Bind a new schedule to the goal its template names, or refuse it.
CREATE OR REPLACE FUNCTION cron_schedules_bind_goal() RETURNS trigger AS $$
DECLARE
    named TEXT;
BEGIN
    IF NEW.task_template IS NULL OR NOT (NEW.task_template ? 'goal_id') THEN
        NEW.goal_id := NULL;
        RETURN NEW;
    END IF;
    IF jsonb_typeof(NEW.task_template -> 'goal_id') <> 'string' THEN
        RAISE EXCEPTION 'thread_goal_schedule_binding_refused' USING ERRCODE = '23514';
    END IF;
    named := NEW.task_template ->> 'goal_id';
    SELECT g.id INTO NEW.goal_id
    FROM thread_goals g
    JOIN threads t ON t.id = g.thread_id
    WHERE g.id = named
      AND g.status = 'active'
      AND g.org_id = NEW.org_id
      AND g.user_id = NEW.creator_subject_id
      AND t.org_id = NEW.org_id
      AND t.user_id = NEW.creator_subject_id
      AND COALESCE(btrim(t.space_id), '') = ''
      AND NOT EXISTS (
          SELECT 1 FROM runs r WHERE r.thread_id = t.id AND r.retention_class = 'zdr_waived'
      );
    IF NEW.goal_id IS NULL THEN
        RAISE EXCEPTION 'thread_goal_schedule_binding_refused' USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS cron_schedules_bind_goal ON cron_schedules;
CREATE TRIGGER cron_schedules_bind_goal
    BEFORE INSERT ON cron_schedules
    FOR EACH ROW EXECUTE FUNCTION cron_schedules_bind_goal();

-- Keep the link fixed, disable a schedule whose goal is gone, and re-enable only for an active goal.
-- A schedule that ran its runs, or whose goal is gone, stays off: the person sets a new one.
CREATE OR REPLACE FUNCTION cron_schedules_guard_goal() RETURNS trigger AS $$
BEGIN
    IF NEW.goal_id IS DISTINCT FROM OLD.goal_id THEN
        IF NEW.goal_id IS NOT NULL THEN
            RAISE EXCEPTION 'thread_goal_schedule_binding_refused' USING ERRCODE = '23514';
        END IF;
        -- The goal was deleted (ON DELETE SET NULL): the schedule stops with it.
        NEW.enabled := false;
        NEW.goal_paused_reason := 'goal_deleted';
        RETURN NEW;
    END IF;
    IF NEW.enabled AND NOT OLD.enabled THEN
        IF OLD.goal_paused_reason IN ('goal_deleted', 'goal_runs_reached') THEN
            RAISE EXCEPTION 'thread_goal_schedule_finished' USING ERRCODE = '23514';
        END IF;
        IF NEW.goal_id IS NOT NULL AND NOT EXISTS (
            SELECT 1 FROM thread_goals WHERE id = NEW.goal_id AND status = 'active'
        ) THEN
            RAISE EXCEPTION 'thread_goal_not_active' USING ERRCODE = '23514';
        END IF;
        NEW.goal_paused_reason := NULL;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS cron_schedules_guard_goal ON cron_schedules;
CREATE TRIGGER cron_schedules_guard_goal
    BEFORE UPDATE ON cron_schedules
    FOR EACH ROW
    WHEN (OLD.goal_id IS NOT NULL OR NEW.goal_id IS NOT NULL
          OR OLD.goal_paused_reason IS NOT NULL)
    EXECUTE FUNCTION cron_schedules_guard_goal();

-- A goal that leaves `active` pauses its schedules in the transaction that moved it.
CREATE OR REPLACE FUNCTION thread_goals_pause_schedules() RETURNS trigger AS $$
BEGIN
    UPDATE cron_schedules
    SET enabled = false,
        goal_paused_reason = CASE WHEN NEW.status = 'paused' THEN 'goal_paused' ELSE 'goal_closed' END,
        updated_at = now()
    WHERE goal_id = NEW.id AND enabled AND deleted_at IS NULL;
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS thread_goals_pause_schedules ON thread_goals;
CREATE TRIGGER thread_goals_pause_schedules
    AFTER UPDATE OF status ON thread_goals
    FOR EACH ROW
    WHEN (OLD.status = 'active' AND NEW.status <> 'active')
    EXECUTE FUNCTION thread_goals_pause_schedules();
