-- 0087: runs.external_content_at, the web-taint marker (WS11.S7).
--
-- Set once, by session-core, when a run (or a delegated child run of it) completes a tool
-- step whose result is external content: web search or fetch, browser output, or a
-- third-party MCP tool (mp_content_safety::TrustClass::is_external_content). Its only
-- consumer is Dreaming, which will not turn an assistant message written inside such a run's
-- window into a durable memory, because the assistant may be paraphrasing a page that
-- carried instructions. User messages stay eligible.
--
-- It is a timestamp derived from a tool NAME. It holds no URL, query, page text or tool
-- result, so a zero-retention run stores nothing but a time (docs/ZDR.md). Additive and
-- nullable: existing rows read as "not tainted", and nothing needs a backfill.

ALTER TABLE runs
    ADD COLUMN IF NOT EXISTS external_content_at TIMESTAMPTZ;

-- Dreaming asks "which runs of this thread were tainted", never the reverse.
CREATE INDEX IF NOT EXISTS idx_runs_thread_external
    ON runs (thread_id)
    WHERE external_content_at IS NOT NULL;
