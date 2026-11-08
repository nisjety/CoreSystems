# Wave 4 verification, quarryv2 (checked 2026-11-07 ~11:50)

## Result: nothing to verify yet. No Wave 4 change exists in this worktree.

- Worktree `.claude/worktrees/gracious-gagarin-087139`, branch `quarryv2`, HEAD `bed81d162` (2026-10-16 17:01).
- `git status` is clean apart from the untracked `.plan-work/`. No staged or modified files. The reflog's last entry is `reset: moving to HEAD` on 2026-10-16.
- No commit on `quarryv2` since 2026-10-16 touches Wave 4 (E4.1 to E4.7, plan section 3.4). The last commits are WS4.A/K, PRE-2, WS2.A, WS6.A to K, WS3.A, WS5.A/B, WS1.A0/A1 and the F1 HMAC fix.
- The cargo/rustc processes running at the time were Model Plane `session-core` tests from another session. They are unrelated.
- Sibling branch `quarryv2-next` (worktree vigorous-perlman-e7ae5b) has INNH work (POST /v1/parse and others), not Wave 4 either.
- So: no touched crates, no `cargo test` run (it would only re-test HEAD), and nothing to re-verify. Re-run once the writer commits or leaves a diff.

## What was READ (not run)

| Check | How | Finding |
|---|---|---|
| Wave 4 definition | read plan section 3.4 (lines 3163 to 3204) | Wave 4 is the E4.x epics. E4.1 slice 1 (result sets as a page index) is the one with no decision needed. |
| Flag defaults in `apps/Ingestion Plane/docker-compose.yml` | grep, read | `QUARRY_JOB_ADMISSION` is `off`. `BUDGET_ENFORCEMENT` is `shadow`. `METRICS_ENABLED` is `0`. `DRAIN_DEADLINE_S` is `30`. `BROWSER_NO_SANDBOX` is `0` (the prod overlay sets `"0"`). All still dark or safe. |
| Wave 4 flags in compose | grep for `AI_FORMATS`, `SCRAPE_OPTIONS`, `SCHEDULED_JOBS`, `page_index`, `RESULT` | None exist yet. `DOC_ROUTING` is in code only (default `shadow`). It is not in either compose file, so its default is set by code. |
| Dataset source | read `PageExtraction` in `quarry-core/src/output.rs:86`; grep for `ScrapeOptions` in Quarry-v2 | No `ScrapeOptions` or `scrape_options` anywhere in Quarry-v2 Rust or Go. The `page_extracted` event has no artifact id and no structured fields. |

## Blockers

1. WS5.I (ScrapeOptions in crawl, batch, search hydration, schedules). Unscheduled, L, 12 to 15 d. It needs WS5.D (the inline scrape contract, not built on this branch, only WS5.A/B are) and WS14.D (not built). Merge order is WS6.A, WS14, WS15.B, then WS5.I. Also needs a control migration after 016 to 020, and ADR-4/ADR-9 for the durable parts.
2. WS16.J (AI formats in crawl and batch). Unscheduled, L, 8 to 10 d (estimate flagged UNVERIFIED). It needs WS5.I, WS15.D, WS6.A, WS6.D. WS6.D is not started (needs the billing-core credential and ADR-1). The whole WS16 lane is also gated by DR-15.1 and the X-7 grant ruling.
3. The dataset source question (plan section 3.4, decision (a), D14b D9). Until crawl and batch embed scrape options with the `json` format (WS5.I plus WS16), E4.1's `GET /v1/runs/{id}/items` can only return a page index (`kind: page_index`). It cannot return a dataset. Crawl and batch are refused under ZDR, so there is no ZDR result set. Wave 4 must ship E4.1 as a page index and label it as one.
