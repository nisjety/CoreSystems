-- WS15.S3 (X-9 / D14 option 2): trigger and backfill as pull-model request rows.
--
-- quarry-control holds no Temporal client. POST /v1/schedules/{id}/trigger and
-- /backfill insert a row here and answer 202; the orchestrator claims rows every
-- 2 s with FOR UPDATE SKIP LOCKED, calls Temporal and posts the result. A row is
-- durable before anything runs, so an orchestrator restart between request and
-- claim loses nothing; a claimed row whose lease ran out is claimed again.
--
-- The plan numbered this 018; 018 was never used and INNH-5 holds 025, so it is 026.
-- Behind QUARRY_CONTROL__SCHEDULE_REQUESTS (off by default): with the flag off no
-- route writes this table. The row holds ids, window times and short error text,
-- never page content.
--
-- Idempotent: every statement is IF NOT EXISTS-safe.

CREATE TABLE IF NOT EXISTS schedule_requests (
    id              TEXT PRIMARY KEY,
    schedule_id     TEXT NOT NULL,
    org_id          TEXT NOT NULL,
    action          TEXT NOT NULL CHECK (action IN ('trigger', 'backfill')),
    params          JSONB NOT NULL DEFAULT '{}'::jsonb,
    status          TEXT NOT NULL DEFAULT 'pending'
                    CHECK (status IN ('pending', 'claimed', 'done', 'failed', 'rejected')),
    claimed_by      TEXT NOT NULL DEFAULT '',
    lease_until     BIGINT NOT NULL DEFAULT 0,
    attempts        INT NOT NULL DEFAULT 0,
    result          JSONB,
    error           TEXT NOT NULL DEFAULT '',
    requested_by    TEXT NOT NULL DEFAULT '',
    idempotency_key TEXT,
    created_at      BIGINT NOT NULL,
    finished_at     BIGINT
);

-- The claim scan: open rows, oldest first.
CREATE INDEX IF NOT EXISTS schedule_requests_open_idx
    ON schedule_requests (created_at, id)
    WHERE status IN ('pending', 'claimed');

-- One open request per schedule, enforced by the database so two quick presses
-- cannot both insert (the handler's HasPending check is only the friendly path).
CREATE UNIQUE INDEX IF NOT EXISTS schedule_requests_one_open_idx
    ON schedule_requests (schedule_id)
    WHERE status IN ('pending', 'claimed');

CREATE UNIQUE INDEX IF NOT EXISTS schedule_requests_org_idem_idx
    ON schedule_requests (org_id, idempotency_key)
    WHERE idempotency_key IS NOT NULL;

CREATE INDEX IF NOT EXISTS schedule_requests_org_idx
    ON schedule_requests (org_id, created_at DESC);
