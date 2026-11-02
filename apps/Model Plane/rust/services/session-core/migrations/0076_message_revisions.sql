-- Per-message revision log for edited and deleted messages (build plan WS12-S4).
--
-- A revision is an append-only row keyed by the message it changes. `messages` rows are never
-- modified except by the one sanctioned purge write in src/space/revisions.rs (an org whose
-- retention rule is `purge` blanks the original text there); in `retain` mode the original stays
-- in `messages.content`, hidden from every product read by the fold in list_conversation,
-- load_thread_messages and the Dreaming loaders. A revision names exactly one message: no nesting
-- and no supersede, which is why this is not the rejected persisted-versions design.
--
-- `content` holds the new text of an edit for a non-ZDR org; it is NULL for a delete and, once a
-- newer revision lands under `purge`, for every older revision. Ids, enums and digests are the
-- only other content-free columns. ON DELETE CASCADE removes the revisions with their message, so
-- thread deletion, GDPR erasure and Space deletion need no extra step.
--
-- Additive: a new table and two indexes, nothing existing is rewritten. The reversal is in
-- revert/0076_message_revisions.down.sql.
CREATE TABLE IF NOT EXISTS message_revisions (
    revision_id                 TEXT PRIMARY KEY,
    org_id                      TEXT NOT NULL,
    thread_id                   TEXT NOT NULL,
    message_id                  TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    revision_no                 INTEGER NOT NULL CHECK (revision_no > 0),
    action                      TEXT NOT NULL CHECK (action IN ('edited', 'deleted')),
    actor_subject_id            TEXT NOT NULL,
    actor_class                 TEXT NOT NULL CHECK (actor_class IN ('author', 'agent_owner', 'moderator')),
    moderation_reason           TEXT NOT NULL DEFAULT ''
        CHECK (moderation_reason IN ('', 'policy_violation', 'spam', 'harassment', 'privacy', 'other')),
    content                     TEXT,
    content_digest              TEXT NOT NULL,
    prev_content_digest         TEXT NOT NULL,
    original_retention          TEXT NOT NULL CHECK (original_retention IN ('retain', 'purge')),
    idempotency_hash            TEXT NOT NULL,
    space_id                    TEXT NOT NULL,
    recipient_audience_revision BIGINT NOT NULL,
    authority_revision          BIGINT NOT NULL,
    resource_authorization_ref  TEXT NOT NULL,
    created_at                  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (message_id, revision_no),
    UNIQUE (message_id, idempotency_hash)
);

CREATE INDEX IF NOT EXISTS idx_message_revisions_thread
    ON message_revisions (thread_id, message_id, revision_no DESC);
