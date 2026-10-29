-- Continuation descriptors are sensitive action input. Keep only the
-- normalized binding fields needed for receipts plus authenticated ciphertext;
-- the descriptor JSON itself must never live in approvals.metadata plaintext.
CREATE TABLE IF NOT EXISTS approval_continuation_descriptors (
    approval_id TEXT PRIMARY KEY
        REFERENCES approvals(id) ON DELETE CASCADE,
    run_id TEXT NOT NULL
        REFERENCES runs(id) ON DELETE CASCADE,
    org_id TEXT NOT NULL
        CHECK (length(org_id) BETWEEN 1 AND 128),
    user_id TEXT NOT NULL
        CHECK (length(user_id) BETWEEN 1 AND 128),
    descriptor_version SMALLINT NOT NULL
        CHECK (descriptor_version > 0),
    action_fingerprint TEXT NOT NULL
        CHECK (length(action_fingerprint) = 64),
    ciphertext TEXT NOT NULL
        CHECK (length(ciphertext) BETWEEN 32 AND 131072),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS approval_continuation_descriptors_scope_idx
    ON approval_continuation_descriptors (org_id, run_id, approval_id);
