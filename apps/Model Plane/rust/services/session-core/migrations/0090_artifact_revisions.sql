-- 0090: the durable revision store of thread artifacts (Wave 3 EP-3 S2).
--
-- The gateway publishes each model revision of an artifact into the assistant
-- message's `metadata.artifacts`, and keeps the version counter in its own
-- process memory. This table gives every revision its own id, its editor and
-- the message it came from, and holds a head that two gateway replicas can
-- compare-and-set against. `metadata.artifacts` is still written as before
-- (the Space room aggregate and rehydration read it); this table sits beside it.
--
-- Zero Data Retention: a zero-retention credential is refused at the RPC and a
-- thread that holds a waived run (DEC-32) never gets a row, so the only content
-- here is content the thread already keeps. Erasure: rows go with their thread
-- (ON DELETE CASCADE, and an explicit statement in delete_thread_rows) and with
-- their org (gdpr.rs purge). `kind` is an open string (integration C-B8: a deck
-- is an artifact kind too), bounded only in shape.
--
-- Additive only. The table is new and empty, so no index needs CONCURRENTLY.

CREATE TABLE IF NOT EXISTS artifact_revisions (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL,
    thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    artifact_id TEXT NOT NULL
        CHECK (char_length(artifact_id) BETWEEN 1 AND 256),
    version INTEGER NOT NULL CHECK (version >= 1),
    kind TEXT NOT NULL CHECK (kind ~ '^[a-z0-9][a-z0-9_-]{0,63}$'),
    title TEXT NOT NULL CHECK (char_length(title) <= 2000),
    content TEXT NOT NULL CHECK (char_length(content) <= 200000),
    editor_kind TEXT NOT NULL CHECK (editor_kind IN ('model', 'user', 'restore')),
    editor_subject_id TEXT NOT NULL,
    run_id TEXT NULL,
    source_message_id TEXT NULL,
    restored_from_version INTEGER NULL
        CHECK (restored_from_version IS NULL OR restored_from_version >= 1),
    content_sha256 TEXT NOT NULL CHECK (content_sha256 ~ '^[0-9a-f]{64}$'),
    -- An opaque caller operation id, never derived from content; a retry with
    -- the same key returns the first row.
    idempotency_key TEXT NULL
        CHECK (idempotency_key IS NULL OR char_length(idempotency_key) BETWEEN 1 AND 128),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (thread_id, artifact_id, version),
    -- A restore names the version it restored; nothing else does.
    CHECK ((editor_kind = 'restore') = (restored_from_version IS NOT NULL))
);

-- The head of one artifact and its history, newest first.
CREATE INDEX IF NOT EXISTS artifact_revisions_head_idx
    ON artifact_revisions (thread_id, artifact_id, version DESC);

-- One row per idempotency key within a thread.
CREATE UNIQUE INDEX IF NOT EXISTS artifact_revisions_idempotency_idx
    ON artifact_revisions (thread_id, idempotency_key)
    WHERE idempotency_key IS NOT NULL;

-- The org purge deletes by org_id.
CREATE INDEX IF NOT EXISTS artifact_revisions_org_idx
    ON artifact_revisions (org_id);
