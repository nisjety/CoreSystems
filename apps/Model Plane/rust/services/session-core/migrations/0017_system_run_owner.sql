-- System-owned runs: a run created by a durable workflow with no human behind it.
--
-- No schema change is REQUIRED for correctness. `runs.user_id` and
-- `threads.user_id` are bare `TEXT NOT NULL` (0001_init.sql) with no foreign key,
-- no CHECK and no width, so `service:orchestrator-core` was already a legal
-- value. What this migration adds is the bound and the index the new access
-- pattern needs.

-- 1. Bound the owner column.
--
-- Matches the 1..128 bound already enforced on
-- `managed_run_terminalization_outbox.user_id` (0015), so a value that can own a
-- run can also key a managed run later without tripping a stricter constraint
-- further down the line.
--
-- NOT VALID first, VALIDATE second: adding a validated CHECK takes an
-- ACCESS EXCLUSIVE lock for a full table scan, and `runs` is hot. NOT VALID
-- applies to new and updated rows immediately and takes only a brief lock;
-- VALIDATE CONSTRAINT then scans under a weaker SHARE UPDATE EXCLUSIVE lock.
ALTER TABLE runs
    ADD CONSTRAINT runs_user_id_len
    CHECK (length(user_id) BETWEEN 1 AND 128) NOT VALID;
ALTER TABLE runs VALIDATE CONSTRAINT runs_user_id_len;

ALTER TABLE threads
    ADD CONSTRAINT threads_user_id_len
    CHECK (length(user_id) BETWEEN 1 AND 128) NOT VALID;
ALTER TABLE threads VALIDATE CONSTRAINT threads_user_id_len;

-- 2. Index for the org-scoped system-run listing.
--
-- `RunService.ListSystemRuns` filters `org_id` plus `user_id = ANY($2)` and
-- paginates on `id DESC`, mirroring how ListRuns pages. Without this the listing
-- is a full scan of `runs` per request, which is the one query in this feature
-- that has no thread_id to narrow on.
CREATE INDEX IF NOT EXISTS runs_org_owner_id_desc_idx
    ON runs (org_id, user_id, id DESC);

-- Deliberately NOT included:
--
--   * No FOREIGN KEY on user_id. There is no users/actors table in session-core
--     and there must not be one — identity is Control Plane's, and importing it
--     here would put a Control-Plane-owned table inside session-core's schema.
--     A reviewer asking for an FK should read this comment first.
--
--   * No CHECK encoding the owner allowlist (e.g. `user_id NOT LIKE 'service:%'
--     OR user_id IN (...)`). The allowlist lives in exactly one place — the
--     `SYSTEM_RUN_OWNERS` constant in `src/auth.rs`, which both the
--     authorization predicate and the listing query bind from. Duplicating it in
--     SQL creates two sources of truth that drift, and the drift direction that
--     matters is a row the query returns but the guard would not admit.
