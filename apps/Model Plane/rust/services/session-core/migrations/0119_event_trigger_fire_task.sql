-- 0119: the task an authorised trigger fire started (Wave 4 EP-7, F-t3-047, slice W4-F T4).
--
-- An authorised fire of a trigger that names a cron schedule starts one scheduled run. capability-core's trigger
-- starter writes the task through the scheduled-run path (the same task and cron_fires rows a cron fire writes, under
-- a fire key that begins `trigger:`), then records here which task it made and when. A fire with task_id NULL has not
-- been started, and a trigger with no schedule never starts one. The task is a pointer and grants no authority: the
-- workflow dispatcher asks Control again, and PrepareScheduledRunThread and the run budget apply, exactly as for a
-- cron fire.
--
-- Zero Data Retention: both columns hold ids and a time, the same kind of value the table already holds. A trigger
-- exists only for a non-ZDR, non-waived member, so no ZDR content reaches them. Erasure: rows still go with their
-- trigger (ON DELETE CASCADE) and the org purge deletes the triggers. The task id names no row by foreign key, so the
-- purge needs no new ordering.
--
-- Additive only: two nullable columns with no default (metadata-only changes, no rewrite) and a partial index on the
-- authorised fires still waiting to start. Every existing fire stays valid with NULL. The reversal is in
-- revert/0119_event_trigger_fire_task.down.sql.

ALTER TABLE event_trigger_fires
    ADD COLUMN IF NOT EXISTS task_id TEXT NULL
        CHECK (task_id IS NULL OR length(task_id) BETWEEN 1 AND 128),
    ADD COLUMN IF NOT EXISTS started_at TIMESTAMPTZ NULL;

-- The starter's claim: the authorised fires with no task yet, oldest first.
CREATE INDEX IF NOT EXISTS event_trigger_fires_unstarted_idx
    ON event_trigger_fires (settled_at, id)
    WHERE status = 'authorized' AND task_id IS NULL;
