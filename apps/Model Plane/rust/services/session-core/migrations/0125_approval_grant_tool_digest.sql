-- 0125: a standing grant binds to the pinned MCP definition digest (build plan WS13 S7, D2-1123).
--
-- A "this thread" or "this run" grant (migration 0062) matched a call on owner, thread, tool,
-- action key and declared class only. For a pinned MCP tool that let a grant made for one tool
-- definition release a changed definition. capability-core's rules already bind to the digest: an
-- allow whose pinned digest differs from the live binding reads as ask. Grants follow the same rule.
--
--   tool_digest   the sha256 (lowercase hex) of the pinned MCP definition the person reviewed when
--                 the grant was made, or '' for a built-in tool. A call is released only by a grant
--                 whose digest equals the digest the call states, so drift means no match.
--
-- Existing rows keep '' and therefore never match an MCP call that states a digest (fail closed).
-- A built-in tool states '' and keeps matching its '' grants exactly as before.
--
-- Drift is no match plus a rebind on the next decision, never an automatic revoke (D-WS13-4): the
-- next granted decision for the same action rewrites the live grant to the definition and class the
-- person just reviewed.
--
-- Zero Data Retention: the column holds a registry digest, never content. Rows are erased with their
-- thread (ON DELETE CASCADE) and with their org (gdpr.rs). Additive only: one defaulted column and
-- one CHECK. The reversal is in revert/0125_approval_grant_tool_digest.down.sql.

ALTER TABLE approval_scope_grants
    ADD COLUMN IF NOT EXISTS tool_digest TEXT NOT NULL DEFAULT '';

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'approval_scope_grants_tool_digest_shape'
    ) THEN
        ALTER TABLE approval_scope_grants
            ADD CONSTRAINT approval_scope_grants_tool_digest_shape
            CHECK (tool_digest = '' OR tool_digest ~ '^[0-9a-f]{64}$');
    END IF;
END$$;
