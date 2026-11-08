-- 0112: thread share snapshots (Wave 3 EP-12 S14; the EP-12 design reserved 0106, but migrations apply in order and
-- 0111 already existed, so it takes the next number above the highest).
--
-- A snapshot is an immutable copy of one thread's active branch, taken by its owner from the S9 export read
-- with the redacted profile (EP-12 D10, ADR docs/decisions/conversation-share-model.md, proposed). Later turns
-- never reach it. Reading one calls no model, connector or tool.
--
-- Zero Data Retention: the body is user content in a new durable store. A zero-retention or waived caller is
-- refused at the RPC (feature `conversation_share`), and a thread with any non-persistent run or in a Space is
-- refused (`thread_not_exportable`), so no ZDR content is ever here. Revoking deletes the body and its digest in
-- the same statement. Erasure: rows go with their thread (ON DELETE CASCADE, and an explicit statement in
-- delete_thread_rows) and with their org (the gdpr.rs purge deletes the threads).
--
-- Additive only: a new, empty table. The reversal is in revert/0112_thread_share_snapshots.down.sql.

CREATE TABLE IF NOT EXISTS thread_share_snapshots (
    id TEXT PRIMARY KEY CHECK (id LIKE 'snap\_%' AND length(id) <= 64),
    org_id TEXT NOT NULL CHECK (length(org_id) BETWEEN 1 AND 128),
    thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    -- The thread owner's user id at the time of the snapshot; the only reader while no grant path exists.
    owner_id TEXT NOT NULL CHECK (length(owner_id) BETWEEN 1 AND 256),
    title TEXT NOT NULL DEFAULT '' CHECK (char_length(title) <= 1000),
    -- The redacted export as JSON; NULL once revoked.
    body JSONB NULL
        CHECK (body IS NULL OR (jsonb_typeof(body) = 'object' AND octet_length(body::text) <= 8388608)),
    -- `sha256:<hex>` of the stored body text; NULL once revoked.
    body_digest TEXT NULL CHECK (body_digest IS NULL OR body_digest ~ '^sha256:[0-9a-f]{64}$'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_at TIMESTAMPTZ NULL,
    -- A live snapshot has its body and digest; a revoked one has neither.
    CHECK ((revoked_at IS NULL) = (body IS NOT NULL)),
    CHECK ((body IS NULL) = (body_digest IS NULL))
);

-- An owner's snapshots (the listing) and the org purge.
CREATE INDEX IF NOT EXISTS thread_share_snapshots_owner_idx
    ON thread_share_snapshots (org_id, owner_id);

-- A thread's live snapshots (the listing, the per-thread cap and the thread erase).
CREATE INDEX IF NOT EXISTS thread_share_snapshots_thread_idx
    ON thread_share_snapshots (thread_id, created_at DESC)
    WHERE revoked_at IS NULL;
