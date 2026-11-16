-- Run-bound agent actions on a thread (build plan WS18-S3).
--
-- Provenance of the two presentation overrides: `title_source` and `pinned_source` say whether a
-- person (or any write before this migration) or a run-bound agent action set the value. A human
-- write sets the source to 'owner'; an agent action never overwrites a value whose source is
-- 'owner'. Both default to 'owner', so every existing row keeps its meaning.
--
-- Card provenance (WS18-S7): `messages.card_run_id` marks an agent card a run wrote, so the assistant
-- reply slot of that run stays open for its final answer. NULL for every other row.
--
-- Receipts: one row per run action idempotency key. It stores the payload digest only, never the
-- arguments or any content. The thread foreign key cascades, so deleting a thread (and every erase
-- path that deletes one) removes its receipts with it.
--
-- Additive: two columns with defaults, one nullable column, one table. No existing writer changes.
ALTER TABLE threads
    ADD COLUMN IF NOT EXISTS title_source TEXT NOT NULL DEFAULT 'owner'
        CHECK (title_source IN ('owner', 'agent')),
    ADD COLUMN IF NOT EXISTS pinned_source TEXT NOT NULL DEFAULT 'owner'
        CHECK (pinned_source IN ('owner', 'agent'));

ALTER TABLE messages ADD COLUMN IF NOT EXISTS card_run_id TEXT;

CREATE INDEX IF NOT EXISTS messages_card_run_idx
    ON messages (thread_id, card_run_id)
    WHERE card_run_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS run_thread_action_receipts (
    org_id          TEXT        NOT NULL,
    run_id          TEXT        NOT NULL,
    idempotency_key TEXT        NOT NULL,
    receipt_id      TEXT        NOT NULL UNIQUE,
    action_id       TEXT        NOT NULL,
    payload_digest  TEXT        NOT NULL CHECK (payload_digest ~ '^sha256:[0-9a-f]{64}$'),
    decision_ref    TEXT        NOT NULL,
    subject_id      TEXT        NOT NULL,
    thread_id       TEXT        NOT NULL REFERENCES threads (id) ON DELETE CASCADE,
    outcome         TEXT        NOT NULL CHECK (outcome IN ('applied', 'noop')),
    message_id      TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (org_id, run_id, idempotency_key)
);

-- The per-run cap: how many receipts of one action a run already holds.
CREATE INDEX IF NOT EXISTS run_thread_action_receipts_run_idx
    ON run_thread_action_receipts (org_id, run_id, action_id);
