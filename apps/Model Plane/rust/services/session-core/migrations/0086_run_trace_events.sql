-- 0086: run_trace_events, the durable run trace (WS10.S6).
--
-- Quarry publishes browser-run trace events on `mp.v1.trace.run.<run_id>`; the
-- `session-core-trace` consumer (trace_nats.rs) validates each one and inserts it here,
-- and `EventLog.QueryEvents` (eventlog_grpc.rs) reads it back. Nothing writes this table
-- unless SESSION_CORE_TRACE_CONSUMER is on, so the migration is inert on its own.
--
-- Identity. `org_id` and `user_id` are copied from the run's own row by the insert, never
-- from the message, so a publisher cannot attribute a row to a tenant or a user the run does
-- not belong to. `ingest_seq` is the server-assigned read order (the paging cursor);
-- `producer_seq` is the producer's own dense per-run counter, so a gap in it is a dropped
-- event and never a reordering. The pair (run_id, producer, producer_seq) is the idempotency
-- key: a redelivered message is a no-op.
--
-- ZDR. A zero-retention run never gets a row: the consumer requires an explicit
-- `"zdr": false` AND `runs.retention_class <> 'zdr'`. The table has no zdr column on purpose.
--
-- The payload is bounded (8192 bytes of its JSON text) and carries receipts, decisions and
-- artifact references, never page content. Rows go with their run (ON DELETE CASCADE), with
-- their org on the GDPR purge, and after SESSION_CORE_TRACE_RETENTION_DAYS (default 30) by
-- the retention sweeper, which reads `received_at`.

CREATE TABLE IF NOT EXISTS run_trace_events (
    ingest_seq BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    event_id TEXT NOT NULL,
    run_id TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    org_id TEXT NOT NULL CHECK (org_id <> ''),
    user_id TEXT NOT NULL DEFAULT '',
    producer TEXT NOT NULL,
    producer_seq BIGINT NOT NULL CHECK (producer_seq > 0),
    event_type TEXT NOT NULL,
    schema_version INTEGER NOT NULL DEFAULT 1,
    occurred_at TIMESTAMPTZ NOT NULL,
    received_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    resource_ref TEXT NOT NULL,
    correlation_id TEXT NOT NULL DEFAULT '',
    causation_id TEXT NOT NULL DEFAULT '',
    request_id TEXT NOT NULL DEFAULT '',
    agent_id TEXT NOT NULL DEFAULT '',
    agent_role TEXT NOT NULL DEFAULT '',
    agent_parent_id TEXT NOT NULL DEFAULT '',
    origin TEXT NOT NULL DEFAULT ''
        CHECK (origin IN ('', 'USER', 'AGENT', 'SCHEDULE', 'WEBHOOK', 'API')),
    payload JSONB NOT NULL DEFAULT '{}'::jsonb
        CHECK (octet_length(payload::text) <= 8192),
    CONSTRAINT run_trace_events_producer_seq_uq UNIQUE (run_id, producer, producer_seq),
    CONSTRAINT run_trace_events_event_id_uq UNIQUE (event_id)
);

-- The read path: one run's events in ingest order, tenant-scoped.
CREATE INDEX IF NOT EXISTS run_trace_events_run_idx
    ON run_trace_events (org_id, run_id, ingest_seq);

-- The retention sweeper's scan.
CREATE INDEX IF NOT EXISTS run_trace_events_received_idx
    ON run_trace_events (received_at);
