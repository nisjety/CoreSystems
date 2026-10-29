-- Cycle 24 / cluster #7 — Durable job history.
--
-- Backs `RecordJobHistoryEvent`, `ListJobEvents`, `ReplayJobEventWindow`.
-- Frontends use this table (via the edge → control list endpoint) to
-- render progress that survives reconnect — no more phase simulation.
--
-- ## Per-run total order
--
-- `seq` is monotonically increasing per `run_id` and producers MUST
-- assign it without gaps for the same run. The unique index
-- `(run_id, seq)` enforces this; an attempted duplicate insert fails
-- the producer's transaction so a buggy emitter can't corrupt the
-- log silently.
--
-- ## Retention
--
-- No TTL at the schema level — retention is a future cycle's policy
-- decision (per-tenant config). For now the table grows; expect ~50
-- events per run × ~100 runs per org per day ≈ 5000 rows/day/org.

CREATE TABLE IF NOT EXISTS quarry_job_history (
    event_id    TEXT         NOT NULL,
    run_id      TEXT         NOT NULL,
    org_id      TEXT         NOT NULL,
    kind        TEXT         NOT NULL,    -- "crawl" | "search" | ...
    stage       TEXT         NOT NULL,    -- "queued" | "running" | ...
    status      TEXT         NOT NULL,    -- "ok" | "warn" | "error"
    seq         BIGINT       NOT NULL,
    completed   INT          NOT NULL DEFAULT 0,
    total       INT,
    discovered  INT          NOT NULL DEFAULT 0,
    queued      INT          NOT NULL DEFAULT 0,
    retries     INT          NOT NULL DEFAULT 0,
    blocks      INT          NOT NULL DEFAULT 0,
    eta         TIMESTAMPTZ,
    ts          TIMESTAMPTZ  NOT NULL,
    payload     JSONB        NOT NULL DEFAULT '{}'::jsonb,
    PRIMARY KEY (run_id, seq)
);

-- Sanity: every event MUST tag its tenant.
ALTER TABLE quarry_job_history
    ADD CONSTRAINT quarry_job_history_org_id_not_empty
    CHECK (org_id <> '');

-- List events for a single run (ordered).
CREATE INDEX IF NOT EXISTS quarry_job_history_run_seq_idx
    ON quarry_job_history (run_id, seq ASC);

-- Org-wide replay window: events between two timestamps for a tenant.
-- The `ts` index is org-prefixed so cross-tenant scans don't happen.
CREATE INDEX IF NOT EXISTS quarry_job_history_org_ts_idx
    ON quarry_job_history (org_id, ts DESC);

-- Filter-by-stage dashboards: count of running/failed/etc. per org.
CREATE INDEX IF NOT EXISTS quarry_job_history_org_stage_idx
    ON quarry_job_history (org_id, stage, ts DESC);
