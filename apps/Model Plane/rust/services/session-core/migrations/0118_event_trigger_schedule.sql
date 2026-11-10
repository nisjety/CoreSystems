-- 0118: event trigger schedule binding (Wave 4 EP-7, F-t3-047, slice W4-F T3).
--
-- A trigger may name one cron schedule: the scheduled run a fire starts. The binding is a pointer and grants no
-- authority. A fire runs under the schedule's own Control-signed consent contract (the WS20 stored consent, its
-- `approval_contract` in the schedule's task template), and Control authorises every fire afresh. session-core
-- stores the id only after it has checked, in the same database, that the schedule is the member's own live
-- schedule in the trigger's org and Space and carries an approval contract (event_triggers.rs). A trigger with no
-- schedule stays record-only.
--
-- Zero Data Retention: the column holds one schedule id, the same kind of id the table already holds. A ZDR or
-- waived caller is refused on every RPC before the column is read or written, so no ZDR content reaches it.
-- Erasure: rows still go with their org. The column names no schedule row by foreign key, so the organisation
-- purge, which deletes the schedules before the triggers, needs no new ordering.
--
-- Additive only: one nullable column with no default (a metadata-only change, no rewrite) and a partial index on
-- the rows that carry it. Every existing trigger row stays valid with NULL. The reversal is in
-- revert/0118_event_trigger_schedule.down.sql.

ALTER TABLE event_triggers
    ADD COLUMN IF NOT EXISTS schedule_id TEXT NULL
        CHECK (schedule_id IS NULL OR length(schedule_id) BETWEEN 1 AND 128);

-- A schedule's triggers, for the fire path's lookup in a later slice.
CREATE INDEX IF NOT EXISTS event_triggers_schedule_idx
    ON event_triggers (schedule_id)
    WHERE schedule_id IS NOT NULL;
