-- F-f05-16 (D-F05-6) Space overlay tombstones: a run's overlay row can record a
-- deletion. A tombstone is an overlay row (run_id set) with deleted = true,
-- content_hash = the Space hash the run saw when it hydrated (never empty, the
-- column is NOT NULL) and size_bytes 0. Idempotent: the entrypoint re-runs every
-- up.sql at each boot, with no ledger. An existing row reads as not deleted,
-- which is what every row written so far was. Only the hash and the flag are
-- stored here, never file content.
ALTER TABLE workspace_files ADD COLUMN IF NOT EXISTS deleted BOOLEAN NOT NULL DEFAULT false;
