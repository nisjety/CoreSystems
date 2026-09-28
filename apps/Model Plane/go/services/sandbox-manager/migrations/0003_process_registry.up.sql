-- S4.2 durable process registry: the rows behind reattachable background
-- processes running inside a Space-scoped sandbox lease.
-- See apps/Frontend Plane/verevonv3/docs/S4_2_PROCESS_REGISTRY_DESIGN_2026-09-13.md §2.
--
-- Ownership split this schema encodes (design §1): sandbox-manager owns
-- these ROWS; execution-core owns the OS process itself. A child dies with
-- its host (bwrap's --die-with-parent), so "durable" here means the
-- registry never lies after a restart — a host that stops heartbeating has
-- its live rows moved to LOST with their output still readable — not that
-- the process survives.
--
-- The database is shared with session-core (DATABASE_URL points at
-- session_core), so both tables carry the sandbox_ prefix to stay
-- unambiguous next to events/runs/approvals.

-- Lease-side gate. Fail-closed default: every lease acquired before Control
-- could grant space:processes must read as not permitted rather than be
-- grandfathered in. Written by AcquireLease (S4.2 step 2) from the verified
-- capability decision; today nothing sets it, so every lease is correctly
-- not permitted.
ALTER TABLE leases
    ADD COLUMN IF NOT EXISTS processes_permitted BOOLEAN NOT NULL DEFAULT FALSE;

-- state stores the ProcessState wire values directly (1 STARTING, 2 RUNNING,
-- 3 EXITED, 4 KILLED, 5 LOST, 6 EXPIRED) rather than a second string
-- encoding, exactly as leases.state does for SandboxLifecycleState. The
-- proto enum added in step 2 must use these same numbers; internal/process's
-- own State constants are the Go-side source of truth until it exists.
--
-- STARTING exists so the registry can reserve the slot — count limits, TTL
-- clamp, lease eligibility — BEFORE the host spawns anything. A spawn
-- failure moves the row straight to EXITED with end_reason 'spawn_failed'.
CREATE TABLE IF NOT EXISTS sandbox_processes (
    id                 TEXT PRIMARY KEY,
    org_id             TEXT NOT NULL,
    space_id           TEXT NOT NULL,
    lease_id           TEXT NOT NULL,
    -- backend_id + host_epoch are the write fence: only the exact
    -- execution-core instance (and boot) that registered a process may
    -- append output or move its state. A restarted host mints a new epoch,
    -- so its predecessor's rows become unwritable rather than interleaved.
    backend_id         TEXT NOT NULL,
    host_epoch         TEXT NOT NULL,
    run_id             TEXT NOT NULL,
    step_id            TEXT NOT NULL DEFAULT '',
    subject_id         TEXT NOT NULL,
    -- Redacted argv only: {"program": "...", "args": ["..."]} after
    -- internal/redact.Command. command_digest is a sha256 the host computes
    -- over the UNredacted argv, so a caller can recognize "the same command"
    -- without this registry ever storing the plaintext.
    command_redacted   JSONB NOT NULL,
    command_digest     TEXT NOT NULL,
    state              SMALLINT NOT NULL,
    exit_code          INTEGER,
    end_reason         TEXT,
    signal_requested   SMALLINT NOT NULL DEFAULT 0,
    term_requested_at  TIMESTAMPTZ,
    cleanup_state      SMALLINT NOT NULL DEFAULT 1,
    ttl_seconds        INTEGER NOT NULL,
    expires_at         TIMESTAMPTZ NOT NULL,
    started_at         TIMESTAMPTZ,
    ended_at           TIMESTAMPTZ,
    last_heartbeat_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Output bookkeeping; the log itself is sandbox_process_output below.
    -- next_seq/retained_bytes are informational mirrors of the host's own
    -- counter and of the rows actually present; retained_from_seq is the
    -- authoritative "first seq still readable" a reader compares its cursor
    -- against to detect a retention gap.
    next_seq           BIGINT NOT NULL DEFAULT 1,
    retained_from_seq  BIGINT NOT NULL DEFAULT 1,
    retained_bytes     BIGINT NOT NULL DEFAULT 0,
    dropped_bytes      BIGINT NOT NULL DEFAULT 0,
    -- A count only. stdin content is never stored anywhere.
    stdin_bytes        BIGINT NOT NULL DEFAULT 0,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (state BETWEEN 1 AND 6),
    CHECK (signal_requested BETWEEN 0 AND 2),
    CHECK (cleanup_state BETWEEN 1 AND 2),
    CHECK (end_reason IS NULL OR end_reason IN (
        'exited', 'signaled', 'ttl_expired', 'lease_released',
        'host_lost', 'spawn_failed', 'output_budget_exhausted')),
    -- State-shape invariant (session-core migration 0015's pattern): a
    -- terminal row always says how it ended, a live row never does. This is
    -- what makes "the registry cannot lie after a restart" a constraint
    -- rather than a convention.
    CHECK (
        (state IN (1, 2) AND ended_at IS NULL AND end_reason IS NULL)
     OR (state IN (3, 4, 5, 6) AND ended_at IS NOT NULL AND end_reason IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS sandbox_processes_space_idx
    ON sandbox_processes (org_id, space_id, id DESC);

CREATE INDEX IF NOT EXISTS sandbox_processes_lease_idx
    ON sandbox_processes (lease_id);

-- The boot-time reconcile and the staleness sweeper both scan live rows
-- only, so both indexes are partial on exactly that predicate.
CREATE INDEX IF NOT EXISTS sandbox_processes_live_idx
    ON sandbox_processes (backend_id, host_epoch) WHERE state IN (1, 2);

CREATE INDEX IF NOT EXISTS sandbox_processes_heartbeat_idx
    ON sandbox_processes (last_heartbeat_at) WHERE state IN (1, 2);

-- Append-only output log. seq is assigned by the host (the single writer per
-- process, enforced by the fence above), which is what lets an ambiguous
-- retry re-send the same batch under the same seqs: the primary key turns it
-- into ON CONFLICT DO NOTHING rather than a duplicate.
--
-- content is already redacted by the host at line boundaries before it is
-- sent (design §3.3) — a secret split across two fixed-size chunks would no
-- longer match any scrub pattern — so this table never re-scrubs.
CREATE TABLE IF NOT EXISTS sandbox_process_output (
    process_id         TEXT NOT NULL REFERENCES sandbox_processes(id) ON DELETE CASCADE,
    seq                BIGINT NOT NULL,
    -- 1 stdout, 2 stderr, 3 registry-authored system marker (no content).
    stream             SMALLINT NOT NULL,
    content            BYTEA NOT NULL,
    -- S4.3's process-output watch adapter has to know whether the last line
    -- of a chunk is complete before it can emit it as an event.
    ends_with_newline  BOOLEAN NOT NULL,
    captured_at        TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (process_id, seq),
    CHECK (stream BETWEEN 1 AND 3)
);
