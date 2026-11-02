-- Quarantined outputs (build plan WS10-S8).
--
-- When a Space run's external tool result matches a prompt-injection marker and the Space
-- holds quarantine in `enforce`, the model is handed a placeholder and the content is kept
-- here, encrypted with the continuation cipher (AAD names the approval, org and person), against
-- an UNTRUSTED_CONTENT approval. A person grants the approval; the agent's retry of the exact
-- call (same tool, same action digest, same thread, same person) is then handed the content
-- once, framed as untrusted data. Deny, expiry and release blank the ciphertext.
--
--   action_digest   sha256 of the canonical {tool, input}; the retry must match it
--   content_hash    BLAKE3 of the held content, for the audit trail
--   consumed_at     the single release; set once, by one atomic update
--   purged_at       set when the ciphertext was blanked (deny, release or expiry)
--
-- Additive: a new table, no existing row or writer changes.
CREATE TABLE IF NOT EXISTS quarantined_outputs (
    approval_id   TEXT        PRIMARY KEY REFERENCES approvals (id) ON DELETE CASCADE,
    run_id        TEXT        NOT NULL REFERENCES runs (id),
    org_id        TEXT        NOT NULL,
    user_id       TEXT        NOT NULL,
    tool_key      TEXT        NOT NULL,
    action_digest TEXT        NOT NULL CHECK (action_digest ~ '^[0-9a-f]{64}$'),
    trust_class   TEXT        NOT NULL,
    content_hash  TEXT        NOT NULL,
    ciphertext    TEXT        NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    released_at   TIMESTAMPTZ,
    consumed_at   TIMESTAMPTZ,
    purged_at     TIMESTAMPTZ,
    expires_at    TIMESTAMPTZ NOT NULL
);

-- The release lookup: a held, unspent output of this person for this exact call.
CREATE INDEX IF NOT EXISTS quarantined_outputs_release_idx
    ON quarantined_outputs (org_id, user_id, tool_key, action_digest)
    WHERE consumed_at IS NULL AND purged_at IS NULL;

-- The expiry sweep.
CREATE INDEX IF NOT EXISTS quarantined_outputs_expiry_idx
    ON quarantined_outputs (expires_at)
    WHERE purged_at IS NULL;
