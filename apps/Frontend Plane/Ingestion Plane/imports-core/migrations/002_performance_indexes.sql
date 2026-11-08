-- Migration: 002_performance_indexes.sql
-- Adds missing indexes for listing and background sweep queries
-- All statements use IF NOT EXISTS — safe to re-run

-- Status-only sweep: background jobs scanning for stale/failed imports
CREATE INDEX IF NOT EXISTS idx_import_jobs_status
  ON import_jobs(status);

-- Listing imports chronologically (default sort for UI)
CREATE INDEX IF NOT EXISTS idx_import_jobs_created_at
  ON import_jobs(created_at DESC);

-- Listing job items ordered by time (used in run_job and get_job_with_items)
CREATE INDEX IF NOT EXISTS idx_import_job_items_created_at
  ON import_job_items(created_at DESC);
