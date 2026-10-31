-- WS15.S1: tenant-safe schedule definition (migration 017).
--
-- A UI-created scrape, crawl or batch schedule stores its whole definition
-- here: a name, the per-kind config, the timezone, the overlap policy, the
-- catch-up window, pause-on-failure and the jitter. The runtime columns
-- (last_run_at, next_run_at, skipped_overlap_count) are written by the
-- executor in WS15.S2; nothing fires from this migration.
--
-- Every column defaults to the value a pre-017 row already means, so an
-- existing row reads back unchanged. Idempotent: every statement is
-- IF NOT EXISTS or a guarded UPDATE, so a manual re-run is a no-op.
--
-- Pre-017 scrape, crawl and batch rows never ran (the orchestrator skipped
-- them: workflowFor maps only change_monitor) and have no stamped
-- updated_at, so they are disabled once. UpdateEnabled stamps updated_at, so
-- a row an operator re-enables is never disabled again by a re-run.
-- change_monitor rows are not touched.

ALTER TABLE schedules ADD COLUMN IF NOT EXISTS name TEXT NOT NULL DEFAULT '';
ALTER TABLE schedules ADD COLUMN IF NOT EXISTS config JSONB NOT NULL DEFAULT '{}';
ALTER TABLE schedules ADD COLUMN IF NOT EXISTS overlap_policy TEXT NOT NULL DEFAULT 'skip';
ALTER TABLE schedules ADD COLUMN IF NOT EXISTS timezone TEXT NOT NULL DEFAULT 'UTC';
ALTER TABLE schedules ADD COLUMN IF NOT EXISTS catchup_window_s INTEGER NOT NULL DEFAULT 0;
ALTER TABLE schedules ADD COLUMN IF NOT EXISTS pause_on_failure BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE schedules ADD COLUMN IF NOT EXISTS jitter_s INTEGER NOT NULL DEFAULT 0;
ALTER TABLE schedules ADD COLUMN IF NOT EXISTS updated_at BIGINT NOT NULL DEFAULT 0;
ALTER TABLE schedules ADD COLUMN IF NOT EXISTS last_run_at BIGINT;
ALTER TABLE schedules ADD COLUMN IF NOT EXISTS next_run_at BIGINT;
ALTER TABLE schedules ADD COLUMN IF NOT EXISTS skipped_overlap_count BIGINT NOT NULL DEFAULT 0;

-- Tenant list: GET /v1/schedules?org_id= is keyset-paged on (created_at, id).
CREATE INDEX IF NOT EXISTS idx_schedules_org_created ON schedules(org_id, created_at DESC);

-- Disable the pre-017 non-monitor rows that were never stamped.
UPDATE schedules SET enabled = FALSE
 WHERE target_kind IN ('scrape', 'crawl', 'batch') AND updated_at = 0;
