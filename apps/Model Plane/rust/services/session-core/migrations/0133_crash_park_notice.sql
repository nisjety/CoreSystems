-- 0133: the crash-park notice's once-only mark (build plan O-02, DEC-F01-10).
--
-- A run a crash parked keeps its park for the park's wait (SESSION_CORE_PARK_TTL_SECONDS, one day). When the
-- park has waited the notice time (SESSION_CORE_CRASH_PARK_NOTICE_SECONDS, four hours by default), the recovery
-- worker writes one RUN_ATTENTION row and stamps this column in the same transaction, so the person is told once.
-- A park that is claimed, resumed or has run out is never stamped, because it is no longer parked.
--
-- Content: one timestamp, no text. Additive: a nullable column with no default and no backfill, so every existing
-- park reads as not yet notified. The reversal is in revert/0133_crash_park_notice.down.sql.

ALTER TABLE run_loop_checkpoints ADD COLUMN IF NOT EXISTS park_notice_sent_at TIMESTAMPTZ;
