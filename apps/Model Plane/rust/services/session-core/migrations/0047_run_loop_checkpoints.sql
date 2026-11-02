-- Durable loop fold store (build plan WS03 S4).
--
-- One row per run: the latest fold the loop wrote at a round boundary, before an
-- effect, at a pause, or at a park. A fold holds the loop's messages and its
-- pending call, so it is stored as AES-GCM ciphertext bound to this row (see
-- loop_checkpoint.rs). The plaintext never reaches the table.
--
-- Latest wins: `seq` is compare-and-set by the writer. A claim moves a paused or
-- parked row to `resuming` under a lease, so exactly one continuation can resume
-- a given pause. The reversal is in revert/0047_run_loop_checkpoints.down.sql.

CREATE TABLE IF NOT EXISTS run_loop_checkpoints (
    run_id TEXT PRIMARY KEY
        REFERENCES runs(id) ON DELETE CASCADE,
    org_id TEXT NOT NULL
        CHECK (length(org_id) BETWEEN 1 AND 128),
    user_id TEXT NOT NULL
        CHECK (length(user_id) BETWEEN 1 AND 128),
    seq BIGINT NOT NULL
        CHECK (seq > 0),
    kind TEXT NOT NULL
        CHECK (kind IN ('round', 'pre_effect', 'pause', 'park')),
    state TEXT NOT NULL
        CHECK (state IN ('active', 'paused', 'parked', 'resuming', 'closed')),
    pause_ref TEXT
        CHECK (pause_ref IS NULL OR length(pause_ref) BETWEEN 1 AND 256),
    park_reason TEXT
        CHECK (park_reason IS NULL OR length(park_reason) BETWEEN 1 AND 64),
    park_until TIMESTAMPTZ,
    authority_digest TEXT NOT NULL
        CHECK (length(authority_digest) BETWEEN 1 AND 128),
    schema_version SMALLINT NOT NULL DEFAULT 1
        CHECK (schema_version > 0),
    segment_no INTEGER NOT NULL
        CHECK (segment_no > 0),
    -- The encrypted fold: at most 4 MiB of plaintext, which base64 and the AEAD
    -- envelope grow to under 6 MiB.
    ciphertext TEXT NOT NULL
        CHECK (length(ciphertext) <= 6291456),
    fold_digest TEXT NOT NULL
        CHECK (length(fold_digest) = 64),
    claim_token_hash TEXT
        CHECK (claim_token_hash IS NULL OR length(claim_token_hash) = 64),
    claim_expires_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS run_loop_checkpoints_state_idx
    ON run_loop_checkpoints (state, updated_at);
