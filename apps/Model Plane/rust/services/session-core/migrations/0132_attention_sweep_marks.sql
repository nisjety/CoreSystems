-- 0132: the attention sweep's durable high-water mark (build plan WS16 S3, F-f04-20).
--
-- One row per sweeper: the start time of the sweeper's last complete pass, one that read every table and wrote
-- every row it found (src/attention_events.rs). The next pass reads back to that time, less nothing and plus an
-- overlap, so a waiting row created while the sweep was down is announced once it is back. The lookback is never
-- shorter than the ten-minute window every pass reads, and never longer than one day: a row created more than a
-- day before the pass is not announced.
--
-- The row is seeded at migration time, so the first pass after the sweep is switched on reads back to the migration,
-- within the cap. A sweeper with no row reads the ten-minute window, as it always has; the seed means a deployed
-- sweep always has one.
--
-- Content: a sweeper name and a timestamp, nothing else. No org, user, thread, run or text is stored, so the row
-- needs no erasure of its own, and Zero Data Retention has nothing to refuse.
--
-- Additive only: a new table with one seeded row. The reversal is in revert/0132_attention_sweep_marks.down.sql.

CREATE TABLE IF NOT EXISTS attention_sweep_marks (
    sweeper TEXT PRIMARY KEY,
    high_water TIMESTAMPTZ NOT NULL
);

INSERT INTO attention_sweep_marks (sweeper, high_water)
VALUES ('run_attention', now())
ON CONFLICT (sweeper) DO NOTHING;
