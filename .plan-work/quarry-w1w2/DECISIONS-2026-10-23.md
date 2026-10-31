# Decisions of 2026-10-23

## Owner-delegated decisions, decided 2026-10-23

The user said "you decide the optimal build"; the coordinating session decided the open questions of `W3-estimate.md` and they are ratified defaults from now on, recorded as "owner-delegated, decided 2026-10-23":

1. ADR-2: the split is ratified. Build 2a (OTP-only handoff). 2b (secrets) stays unbuilt and gated until the key ceremony and counsel.
2. knowledge.crawl_site (E3.7.9): approval is required above 50 pages; at or below 50 it proceeds under the run budget. The threshold is a tighten-only config, default 50.
3. Office hybrid path (E3.6-6): approved.
4. Fail-closed PII (E3.6-4): approved. A page that fails redaction is dropped with a typed per-page failure; the crawl or batch continues; a partial or unredacted page is never emitted.
5. D9: the approval card renders inline in the transcript and the @tab token renders as an inline chip; no dock is needed (unblocks the swap in W3-B1-06 and E3.3-c).
6. OD13 (E3.2-e1): the consent pattern language is approved as drafted and ships flag-off until counsel reviews the wording; it is not enabled.
7. Migration 021 carries E3.5-1 and the E3.5-6 tag column in one migration; 025 is not used.
8. WS1.A2 is treated as built at integration level. The W2 queue gate on it is lifted for CODE work (E3.1-b, E3.2-e1, E3.7.10), behind default-off flags. No production-effect or deployment claim until X-1 (Control sign-off) and an L2 run exist.

Build rules of the same date: one wave at a time, wave mode (no review-fix loops, no per-slice docker rebuilds or live proofs, one light test pass at the end of the wave), Haiku 5.5 for mechanical slices and Sonnet 5.5 high for authority, audit, ZDR, money and data-loss slices, no Opus, at most 3 agents at a time, usage checked between batches.
