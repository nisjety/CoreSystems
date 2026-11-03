-- 0101_run_request_snapshots.sql
--
-- Request snapshots (build plan EP-14 S6; reserved as 0073 in round A, renumbered to 0101 by round B C-B2).
-- What one model call of a run was sent and offered: the parameters, the routed model, the system prompt, the
-- tool catalogue and a manifest of the messages without their bodies (decision D2 (b)).
--
--   run_request_snapshots        one row per model call. `snapshot` is the request-snapshot-v1 JSON
--                                (mp_contracts::request_snapshot): digests, parameters and the message
--                                manifest, never a message body. `(run_id, call_seq)` is unique.
--   run_request_snapshot_blobs   the system prompt and the tool catalogue, stored once per run by the sha256 of
--                                their full text and referenced by digest from every call that repeats them.
--                                `body` is capped (256 KiB system, 512 KiB tools); `truncated` says it was cut.
--
-- Content. The blobs hold prompt text (platform text, org instructions, memory block, persona) and tool schemas.
-- Only inference-core writes them (RunLedgerService.RecordRequestSnapshot), only while
-- SESSION_CORE_REQUEST_SNAPSHOTS_ENABLED is on, so the migration is inert on its own.
--
-- ZDR. A zero-retention run and a waived run (DEC-32) never get a row: inference-core sends nothing for them and
-- the writer refuses a run whose retention_class is not 'persistent' with zdr_durable_mutation_forbidden. The
-- tables have no zdr column on purpose.
--
-- Retention. `expires_at` is set by the writer from SESSION_CORE_REQUEST_SNAPSHOT_TTL_DAYS (default 7, clamped
-- 1-30); the sweep deletes expired rows while SESSION_CORE_REQUEST_SNAPSHOTS_SWEEP_ENABLED is on. Rows go with
-- their run (ON DELETE CASCADE, so with thread delete) and with their org on the GDPR purge, which deletes them
-- by `org_id` before `runs`. A dropped snapshot is counted on the run as `runs.metadata.snapshots_dropped`, so
-- there is no column for it. `org_id` and `user_id` are copied from the run's own row, never from the caller.
--
-- Additive and idempotent. The reversal is in revert/0101_run_request_snapshots.down.sql.

CREATE TABLE IF NOT EXISTS run_request_snapshots (
    id            TEXT PRIMARY KEY CHECK (length(id) > 0),
    run_id        TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    org_id        TEXT NOT NULL CHECK (org_id <> ''),
    user_id       TEXT NOT NULL DEFAULT '',
    call_seq      INTEGER NOT NULL CHECK (call_seq > 0),
    call_purpose  TEXT NOT NULL DEFAULT 'unspecified',
    request_id    TEXT NOT NULL DEFAULT '',
    snapshot      JSONB NOT NULL,
    system_digest TEXT NOT NULL DEFAULT '',
    tools_digest  TEXT NOT NULL DEFAULT '',
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at    TIMESTAMPTZ NOT NULL,
    UNIQUE (run_id, call_seq)
);

CREATE INDEX IF NOT EXISTS run_request_snapshots_org_expiry_idx
    ON run_request_snapshots (org_id, expires_at);

CREATE INDEX IF NOT EXISTS run_request_snapshots_expiry_idx
    ON run_request_snapshots (expires_at);

CREATE TABLE IF NOT EXISTS run_request_snapshot_blobs (
    run_id     TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    digest     TEXT NOT NULL CHECK (length(digest) = 64),
    kind       TEXT NOT NULL CHECK (kind IN ('system', 'tools')),
    org_id     TEXT NOT NULL CHECK (org_id <> ''),
    body       TEXT NOT NULL,
    bytes      INTEGER NOT NULL CHECK (bytes >= 0),
    truncated  BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (run_id, digest)
);

CREATE INDEX IF NOT EXISTS run_request_snapshot_blobs_org_expiry_idx
    ON run_request_snapshot_blobs (org_id, expires_at);

CREATE INDEX IF NOT EXISTS run_request_snapshot_blobs_expiry_idx
    ON run_request_snapshot_blobs (expires_at);
