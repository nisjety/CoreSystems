-- The chat page's own principle -- "the chat page IS Verevon; a thread created
-- outside it must never appear in it" -- was enforced by inferring scope from
-- space_id emptiness. That predicate only distinguishes Space-scoped threads
-- from everything else; it says nothing about *why* an unscoped thread exists.
-- Two concrete leaks follow directly: the Agent Run Console and Support-assist
-- both invoke without a thread/session key, mint a real unscoped thread through
-- the ordinary create path, and the chat listing includes it because nothing
-- about it looks any different from an actual new chat.
--
-- `origin` makes the surface that created a thread a stored fact instead of an
-- inference. It is set once at creation and never changes -- a thread does not
-- become "chat" because the user later opens it from the chat page.

ALTER TABLE threads
    ADD COLUMN IF NOT EXISTS origin TEXT;

-- Backfill existing rows before the NOT NULL constraint below. A space-scoped
-- thread is unambiguous; a support thread is identified by the same session-key
-- convention `support_thread_id()` already uses at creation (see grpc.rs) so a
-- historical support thread is classified correctly rather than defaulting to
-- 'chat'. Everything else predates this column and was, in practice, chat.
UPDATE threads
SET origin = CASE
    WHEN space_id IS NOT NULL THEN 'space'
    WHEN session_key LIKE 'support\_________-____-____-____-____________'
        ESCAPE '\' THEN 'support'
    ELSE 'chat'
END
WHERE origin IS NULL;

ALTER TABLE threads
    ALTER COLUMN origin SET NOT NULL,
    ALTER COLUMN origin SET DEFAULT 'chat';

ALTER TABLE threads
    ADD CONSTRAINT threads_origin_chk CHECK (
        origin IN ('chat', 'space', 'agent_run', 'support', 'system')
    );

-- A space-scoped thread must carry origin='space' and vice versa: origin is
-- meant to be one additional fact about a thread, not a second, driftable copy
-- of what space_id already states. Anything else is a contradiction between
-- the two columns, not a valid row.
ALTER TABLE threads
    ADD CONSTRAINT threads_origin_space_consistency_chk CHECK (
        (origin = 'space') = (space_id IS NOT NULL)
    );

-- The chat listing's real predicate, going forward, is `origin = 'chat'`
-- rather than an inferred "space_id is empty". Index it as such.
CREATE INDEX IF NOT EXISTS idx_threads_org_user_origin
    ON threads (org_id, user_id, origin, created_at);
