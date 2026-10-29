-- 0012_run_watch_subscriptions.up.sql
--
-- Backs AUTO-2 ("notify me when a run finishes"): lets a user register a
-- watch on one run (a chat turn or agent execution tracked by
-- session-core/orchestrator-core) so that when that run reaches a terminal
-- RUN_COMPLETED/RUN_FAILED event on `mp.v1.run.{run_id}.event`,
-- internal/runwatch's consumer calls notification-core's delegated
-- notification-requests API for every user who watched it.
--
-- One row per (org, run, user): the partial unique index below is what makes
-- the create endpoint's upsert idempotent — POSTing the same watch twice
-- never stacks a second pending row (and therefore never double-notifies).
-- status starts 'pending' and the consumer flips it to 'notified' once the
-- delegated notification-core call succeeds; deleted_at soft-deletes an
-- unsubscribe (DELETE /watchers) without erasing the audit trail of a watch
-- that already fired, and falls outside the unique index so re-subscribing
-- after a delete is not blocked by the old row.
CREATE TABLE IF NOT EXISTS run_watch_subscriptions (
    id           TEXT PRIMARY KEY,
    org_id       TEXT NOT NULL,
    run_id       TEXT NOT NULL,
    user_id      TEXT NOT NULL,
    status       TEXT NOT NULL DEFAULT 'pending',
    event_type   TEXT,
    notified_at  TIMESTAMPTZ,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at   TIMESTAMPTZ
);

CREATE UNIQUE INDEX IF NOT EXISTS run_watch_subscriptions_run_user_uq
    ON run_watch_subscriptions (org_id, run_id, user_id) WHERE deleted_at IS NULL;

-- The consumer's hot lookup on every terminal run event landing on
-- mp.v1.run.*.event (every run in the plane, not just watched ones): narrow
-- to this run's still-pending, non-deleted watchers only.
CREATE INDEX IF NOT EXISTS run_watch_subscriptions_pending_idx
    ON run_watch_subscriptions (run_id, status) WHERE deleted_at IS NULL AND status = 'pending';
