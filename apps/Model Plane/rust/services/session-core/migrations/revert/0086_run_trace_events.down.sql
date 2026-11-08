-- Revert 0086: drops the run trace. The trace is a derived, 30-day copy of what Quarry
-- published; the stream (48 h) is not replayable that far back, so a revert loses it.
DROP TABLE IF EXISTS run_trace_events;
