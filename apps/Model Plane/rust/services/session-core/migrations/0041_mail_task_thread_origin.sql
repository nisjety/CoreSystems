-- Widens the `origin` enum migration 0034 introduced with a 6th value:
-- `mail_task`, for the thread an inbound-email-triggered turn creates
-- (chat-wave2 mail-task spec §3.3, owner ruling 2026-09-28: Postmark inbound,
-- self-forward only). Like `agent_run`, a mail-task thread has no live
-- session key an origin can be inferred from -- the caller declares it
-- explicitly (see grpc.rs's `resolve_thread_origin` and `THREAD_ORIGINS`) --
-- and, also like `agent_run`, it must never require a `space_id`: the
-- existing `threads_origin_space_consistency_chk` constraint from 0034
-- already enforces that for every non-'space' origin, so it needs no change
-- here.
--
-- Migration numbering note: this repo's migrations end at 0040 as of
-- 2026-09-28. This mail-task change claims 0041; a concurrently-developed
-- persona spec is expected to claim 0042 after this lands (see
-- chat-wave2-specs-2026-09-28/00-critique-and-sequence.md §1, Tier 3).
ALTER TABLE threads
    DROP CONSTRAINT IF EXISTS threads_origin_chk;

ALTER TABLE threads
    ADD CONSTRAINT threads_origin_chk CHECK (
        origin IN ('chat', 'space', 'agent_run', 'support', 'system', 'mail_task')
    );
