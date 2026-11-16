-- Declassified memory ledger (build plan WS17-S7, plan 4.6 number 0083; design WS17 D5).
--
-- One row per share of a personal memory into a shared Space. Hashes and ids only: the reviewed text
-- lives in the room row (agent_memory, origin_kind 'declassified'), never here.
--
--   decision_ref     the single-use key of the signed Control model.memory.declassify decision
--   idempotency_key  the client request id; (org, subject, Space, key) is unique, so a retry with the
--                    same key returns the first room row and writes no second ledger row
--   retracted_at     set when the declassifying subject removes the room copy (SD-22: the declassifying
--                    owner retracts, room managers do not). The row stays, for audit, and blocks a replay
--
-- No foreign key to agent_memory: a retraction deletes the room row and the ledger outlives it.
-- Reversal: revert/0083_memory_declassifications.down.sql.

CREATE TABLE IF NOT EXISTS memory_declassifications (
    decision_ref      TEXT PRIMARY KEY CHECK (decision_ref <> ''),
    org_id            TEXT NOT NULL CHECK (org_id <> ''),
    space_ref         TEXT NOT NULL CHECK (space_ref <> ''),
    source_memory_id  TEXT NOT NULL CHECK (source_memory_id <> ''),
    room_memory_id    TEXT NOT NULL CHECK (room_memory_id <> ''),
    subject_id        TEXT NOT NULL CHECK (subject_id <> ''),
    idempotency_key   TEXT NOT NULL CHECK (idempotency_key <> ''),
    content_digest    TEXT NOT NULL CHECK (content_digest ~ '^sha256:[0-9a-f]{64}$'),
    audience_hash     TEXT NOT NULL CHECK (audience_hash <> ''),
    audience_revision BIGINT NOT NULL CHECK (audience_revision > 0),
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    retracted_at      TIMESTAMPTZ
);

CREATE UNIQUE INDEX IF NOT EXISTS memory_declassifications_idem_uq
    ON memory_declassifications (org_id, subject_id, space_ref, idempotency_key);

CREATE INDEX IF NOT EXISTS memory_declassifications_room_idx
    ON memory_declassifications (org_id, space_ref, room_memory_id);
