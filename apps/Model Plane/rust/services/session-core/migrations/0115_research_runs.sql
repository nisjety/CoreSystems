-- 0115: research run records (Quarry v2 E4.6, slice Q4-062b; plan 3.4 E4.6 scope (b)).
--
-- A row is one research phase of one chat turn, kept so that it can be addressed by id: its status, the budget it
-- was given, what it has spent and read so far, its stop reason and halt code and, for a run that keeps content, its
-- partial results and the id of its report artifact. The chat run it belongs to is named by run_id, and its thread
-- by thread_id. The store is src/research_runs.rs.
--
-- Status: running is the only state that moves. A run ends in one terminal status, and a terminal status is final.
-- A thread has at most one running run (the partial unique index below).
--
-- Zero Data Retention: a ZDR run, and a run under a connection-bound waiver, is stored with zdr = true and keeps ids,
-- status, codes and counters only. The CHECK refuses a partial result or a report artifact on such a row, and the
-- store drops both before it writes, so no content is stored even by a caller that forgets the rule.
--
-- No foreign keys. A ZDR turn has no thread row, so thread_id names nothing, and run_id names the chat run without a
-- cascade. The thread erase (grpc.rs delete_thread_rows, which the retention sweep also calls) deletes a thread's rows,
-- and the org purge (gdpr.rs) deletes an organisation's rows.
--
-- Additive only: a new, empty table. The reversal is in revert/0115_research_runs.down.sql.

CREATE TABLE IF NOT EXISTS research_runs (
    id TEXT PRIMARY KEY CHECK (length(id) BETWEEN 1 AND 128),
    org_id TEXT NOT NULL CHECK (length(org_id) BETWEEN 1 AND 128),
    user_id TEXT NOT NULL CHECK (length(user_id) BETWEEN 1 AND 256),
    thread_id TEXT NOT NULL CHECK (length(thread_id) BETWEEN 1 AND 128),
    run_id TEXT NOT NULL CHECK (length(run_id) BETWEEN 1 AND 128),
    request_id TEXT NULL CHECK (request_id IS NULL OR length(request_id) BETWEEN 1 AND 128),
    status TEXT NOT NULL CHECK (status IN ('running', 'completed', 'stopped', 'failed', 'cancelled', 'expired')),
    stop_reason TEXT NULL CHECK (stop_reason IS NULL OR length(stop_reason) BETWEEN 1 AND 64),
    halt_code TEXT NULL CHECK (halt_code IS NULL OR length(halt_code) BETWEEN 1 AND 64),
    zdr BOOLEAN NOT NULL,
    budget_pages INT NULL CHECK (budget_pages IS NULL OR budget_pages >= 0),
    budget_sub_queries INT NULL CHECK (budget_sub_queries IS NULL OR budget_sub_queries >= 0),
    budget_usd_micro BIGINT NULL CHECK (budget_usd_micro IS NULL OR budget_usd_micro >= 0),
    spent_usd_micro BIGINT NOT NULL DEFAULT 0 CHECK (spent_usd_micro >= 0),
    pages_read INT NOT NULL DEFAULT 0 CHECK (pages_read >= 0),
    sources_found INT NOT NULL DEFAULT 0 CHECK (sources_found >= 0),
    partial JSONB NULL,
    report_artifact_id TEXT NULL CHECK (report_artifact_id IS NULL OR length(report_artifact_id) BETWEEN 1 AND 128),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deadline_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT research_runs_zdr_keeps_no_content
        CHECK (zdr = FALSE OR (partial IS NULL AND report_artifact_id IS NULL))
);

-- At most one running run per thread. begin expires a stale run first and then refuses while one remains; this index
-- is the backstop for that rule.
CREATE UNIQUE INDEX IF NOT EXISTS research_runs_one_running_idx
    ON research_runs (org_id, thread_id) WHERE status = 'running';

-- A thread's rows: the thread erase and the retention sweep.
CREATE INDEX IF NOT EXISTS research_runs_thread_idx
    ON research_runs (thread_id);

-- An organisation's rows: the org purge.
CREATE INDEX IF NOT EXISTS research_runs_org_idx
    ON research_runs (org_id);

-- The expiry sweep: running runs by deadline.
CREATE INDEX IF NOT EXISTS research_runs_deadline_idx
    ON research_runs (deadline_at) WHERE status = 'running';
