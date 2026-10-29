-- Cycle 20 / cluster #1 — Postgres-backed durable request queue + run checkpoints.
--
-- Provides crash-safe crawl/scrape resumption:
--   * Workers cannot re-fetch URLs that already shipped (idempotency on
--     (org_id, request_id)).
--   * In-flight items reappear automatically after `visibility_deadline_at`
--     so a worker crash never strands a request.
--   * `quarry_run_checkpoints` snapshots `FrontierCheckpoint` (JSONB) so a
--     resumed orchestrator picks up at the last good frontier state.
--   * `quarry_retry_events` records every retry decision the runtime
--     made so operators can audit poison-pill / flaky-host behavior.
--
-- Tenant isolation: every table carries `org_id` (NOT NULL) — the
-- application enforces P0 / cluster #auth+tenancy by populating this
-- field from the verified JWT claim. The composite indexes below are
-- always prefixed on `org_id` so the planner picks tenant-local pages
-- without scanning peer-tenant rows.

-- =============================================================================
-- quarry_request_queues — logical queue per crawl / batch / scrape job.
-- =============================================================================
CREATE TABLE IF NOT EXISTS quarry_request_queues (
    queue_id           UUID         PRIMARY KEY,
    org_id             TEXT         NOT NULL,
    name               TEXT         NOT NULL,
    -- "crawl" | "batch" | "scrape" | "search" — informational, not enforced.
    kind               TEXT         NOT NULL,
    created_by_user_id TEXT,
    created_at         TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    -- Soft-deleted queues stay around for audit; readers must filter.
    deleted_at         TIMESTAMPTZ,
    -- Free-form bag for per-queue settings (max_depth, max_pages,
    -- include/exclude patterns, etc.). Always JSONB so we can grow
    -- without a migration.
    config             JSONB        NOT NULL DEFAULT '{}'::jsonb
);

CREATE INDEX IF NOT EXISTS quarry_request_queues_org_active_idx
    ON quarry_request_queues (org_id, created_at DESC)
    WHERE deleted_at IS NULL;

-- =============================================================================
-- quarry_queue_items — individual URLs / scrape jobs waiting to be processed.
-- =============================================================================
-- Lifecycle:
--   queued        → pop()          → in_flight
--   in_flight     → ack()          → acked    (terminal)
--   in_flight     → nack()/expiry  → queued   (attempt+=1)
--   any           → fail_perm()    → failed   (terminal)
CREATE TABLE IF NOT EXISTS quarry_queue_items (
    -- Application-supplied dedup key; `(org_id, request_id)` unique.
    request_id            TEXT         NOT NULL,
    queue_id              UUID         NOT NULL REFERENCES quarry_request_queues(queue_id) ON DELETE CASCADE,
    org_id                TEXT         NOT NULL,
    url                   TEXT         NOT NULL,
    priority              SMALLINT     NOT NULL DEFAULT 1, -- 0=low, 1=default, 2=high
    payload               JSONB        NOT NULL DEFAULT '{}'::jsonb,
    status                TEXT         NOT NULL DEFAULT 'queued',
    attempt               INT          NOT NULL DEFAULT 0,
    enqueued_at           TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    -- Set when status='in_flight'; cleared on ack / nack.
    in_flight_since       TIMESTAMPTZ,
    -- Reaper polls for `status='in_flight' AND visibility_deadline_at < NOW()`.
    visibility_deadline_at TIMESTAMPTZ,
    acked_at              TIMESTAMPTZ,
    failed_at             TIMESTAMPTZ,
    failure_reason        TEXT,
    PRIMARY KEY (org_id, request_id),
    -- Sanity: priority must be one of the three known values.
    CHECK (priority IN (0, 1, 2)),
    CHECK (status IN ('queued', 'in_flight', 'acked', 'failed'))
);

-- Hot path: pop() — find the highest-priority queued item in the org.
-- We sort by (priority DESC, enqueued_at ASC) and use SELECT FOR UPDATE
-- SKIP LOCKED so concurrent workers pick disjoint rows without
-- contending on the same row lock.
CREATE INDEX IF NOT EXISTS quarry_queue_items_pop_idx
    ON quarry_queue_items (org_id, queue_id, priority DESC, enqueued_at ASC)
    WHERE status = 'queued';

-- Reaper path: find in-flight items whose visibility deadline has passed.
CREATE INDEX IF NOT EXISTS quarry_queue_items_reap_idx
    ON quarry_queue_items (visibility_deadline_at)
    WHERE status = 'in_flight';

-- Stats / dashboards: fast count() per (queue, status).
CREATE INDEX IF NOT EXISTS quarry_queue_items_stats_idx
    ON quarry_queue_items (queue_id, status);

-- =============================================================================
-- quarry_run_checkpoints — last-known FrontierCheckpoint per run.
-- =============================================================================
-- One row per (org_id, run_id) — UPSERTed every checkpoint_every_n pages.
-- Resumption: orchestrator activity reads the row, deserializes `state`,
-- and replays the crawl from that frontier without re-fetching.
CREATE TABLE IF NOT EXISTS quarry_run_checkpoints (
    run_id     TEXT         NOT NULL,
    org_id     TEXT         NOT NULL,
    queue_id   UUID         REFERENCES quarry_request_queues(queue_id) ON DELETE SET NULL,
    -- Serialized FrontierCheckpoint (config + queue + seen + visited_count).
    state      JSONB        NOT NULL,
    -- Bumps every save so dashboards can show "last saved 12s ago".
    version    BIGINT       NOT NULL DEFAULT 1,
    saved_at   TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    PRIMARY KEY (org_id, run_id)
);

-- Recent-checkpoint dashboard query.
CREATE INDEX IF NOT EXISTS quarry_run_checkpoints_recent_idx
    ON quarry_run_checkpoints (org_id, saved_at DESC);

-- =============================================================================
-- quarry_retry_events — audit trail of every retry decision the runtime made.
-- =============================================================================
-- Append-only. Each row says: "at time T, request_id R failed with
-- error_code E and we decided to <retry_class> after <delay_ms>". Joined
-- with quarry_queue_items via (org_id, request_id) when investigating a
-- specific stuck job.
CREATE TABLE IF NOT EXISTS quarry_retry_events (
    event_id      BIGSERIAL    PRIMARY KEY,
    org_id        TEXT         NOT NULL,
    request_id    TEXT         NOT NULL,
    run_id        TEXT,
    attempt       INT          NOT NULL,
    error_code    TEXT         NOT NULL,
    retry_class   TEXT         NOT NULL,  -- "transient" | "rate_limited" | "blocked" | "permanent"
    delay_ms      INT          NOT NULL DEFAULT 0,
    observed_at   TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    note          TEXT
);

CREATE INDEX IF NOT EXISTS quarry_retry_events_by_request_idx
    ON quarry_retry_events (org_id, request_id, observed_at DESC);

CREATE INDEX IF NOT EXISTS quarry_retry_events_by_run_idx
    ON quarry_retry_events (run_id, observed_at DESC)
    WHERE run_id IS NOT NULL;
