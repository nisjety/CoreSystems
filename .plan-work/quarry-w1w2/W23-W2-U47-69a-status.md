# W2-U47-69a status: session controller tests, Solid 2 flush fix, GAP sitemap rows, absent-scope note (lane c, branch w23/lane-c)

## Built / partly

Check first: the controller test file already existed with 10 cases (open then close, ZDR never sends a profile, the profile on a non-ZDR session, busy gating, a superseded open, a stale timeline reply, the navigation card, the non-navigation error, approvals, the page-hide DELETE). Those are unchanged. This unit builds the gap only.

| Scope item | State | Where |
|---|---|---|
| Solid 2 flush hazard (D2-175): the controller's signals read through `latest()` | Built | `use-browser-session-controller.ts`, import from `@/shared/lib/create-immediate-signal` (`3e47e5725`) |
| Stale replies cannot write into the session that is current now: tab action error, control-change error and stale notice, approval decision (listing, decision, optimistic update, error), busy release of a replaced session's action | Built (leak fix, no flag) | `use-browser-session-controller.ts`: `applyTabs`, `setControlMode`, `decideApproval`, `releaseBusy` (`3e47e5725`) |
| Same-tick reads with no flush: `busy()` after `runAction`, `preview()` after `close()` | Built | `use-browser-session-controller.test.ts` (`08e49bb3b`) |
| Stale action reply after a replacement; failed action after a close | Built | same |
| Tab error after a replacement; a replaced session's action must not clear the new session's busy gate | Built | same |
| Open settling after `close()` and after `dispose()` is closed and never attached | Built | same |
| GAP.md rows 177 and 201: seeding described as built and flag-gated (default off); status stays Open | Built | `apps/Ingestion Plane/Quarry-v2/docs/GAP.md` (`bec2e32d6`) |
| CRAWL_SCOPE_ROBOTS.md section 13: what an absent scope does under each `scope_mode` (D2-073 default); scheduled-crawl gap named | Built | `apps/Ingestion Plane/Quarry-v2/docs/CRAWL_SCOPE_ROBOTS.md` (`d0a528e03`) |

Partly built: none. Deliberately not built: the `dispose()` close rule (D2-209, the owner's call) and the scheduled-crawl refusal (W23-b-U64-1, in lane b's files `inputs.go` and `scheduled.go`).

## Commits (lane c, local, not pushed)

- `3e47e5725` fix(verevon-v3): browser session controller reads through the immediate signal (D2-175)
- `08e49bb3b` test(verevon-v3): session controller cases for same-tick reads and stale replies (WS9.F)
- `bec2e32d6` docs(quarry): GAP sitemap rows say seeding is built and flag-gated (D2-037)
- `d0a528e03` docs(quarry): absent-scope crawl behaviour and the scheduled-crawl gap (D2-073)
- The commit that adds this status file and the lane C depth rows is the last one on this branch.

## Code only, not compiled

- Nothing was compiled, type-checked or run: no vitest, no tsc, no eslint, no build (owner order for unit work; the integration step compile-checks).
- Files touched: the controller, its test file, and the two docs. Line endings kept: the `.ts` files are LF (pinned `eol=lf`), the docs are CRLF in the worktree.
- Not verified:
  - That `latest()` in solid-js 2.0.0-rc.0 returns a same-tick pending write. The helper's own comment and the depth notes say so. The lane has no `node_modules`, so the installed typings were not read. The two same-tick cases assert the behaviour directly and fail if it is wrong.
  - The typing of the immediate `createSignal` in the controller (the helper casts to `typeof createPendingSignal`). The same call pattern is used by `anbud-state.ts`, so no new typing pattern is introduced.
  - The existing 10 cases against the new code paths (busy gate, preview, approvals). None should change behaviour, but nothing ran.
  - No docs test reads GAP.md or CRAWL_SCOPE_ROBOTS.md: a grep under Quarry-v2 for both names matched only Rust comments.

## Flag and default

None.

- The controller has no production consumer yet (WS9.G is the first), so the flush fix and the stale guards change no running behaviour today.
- The stale guards are bug and leak fixes, so the flag rule does not apply.
- The docs describe existing flags and do not change any default: `QUARRY_EDGE__CRAWL_SITEMAP` (off) and the crawl scope switch.

## Assumed decisions

1. The "settled accessor" in D2-175 and SPA-B2-04 is read as the shared `create-immediate-signal` helper, whose reads go through `latest()`. The Anbud and Pulse modules already use it. This rests on the helper's comment and its call sites, not on the installed Solid API.
2. The flush fix is the import change alone. Every signal in the controller gets same-tick reads, and no caller changes.
3. The stale-reply guard is extended beyond the three cases the scope names to the error, approval and busy paths. The same leak exists there (a reply from a replaced session writing to the new one). Treated as a leak fix, so no flag.
4. `dispose()` keeps the ZDR-only close. D2-209 is the owner's decision and is not changed here.
5. GAP rows 177 and 201 keep status Open: sitemap seeding is built behind a flag, off by default, and left off until the owner enables it (INNH-6). Rows 292 and 419 repeat the old claim ("CrawlJobWF has no sitemap seeding"); they are outside the named rows and were left for the owner.
6. CRAWL_SCOPE_ROBOTS.md section 13 documents the current code, not the plan. The scheduled-crawl gap is recorded as open, not fixed, because the fix belongs in lane b's `inputs.go` and `scheduled.go`.
7. The new tests use the existing module mocks and `ApiError`. No new mock or helper was added.

## MP asks

None. No Model Plane file was touched and `MP-ASKS.md` is unchanged. The controller's AI-run path uses the gateway's durable-run client, and nothing in this unit needs a Model Plane change.

## Depth

Appended to `.plan-work/quarry-w1w2/W23-depth-lane-c.md` as rows for W2-U47-69a. In short: the `dispose()` rule (D2-209), the remaining D2-285 test gaps, `open()` releasing busy while the replaced session's action still runs, the scheduled-crawl refusal gap (W23-b-U64-1), the unwired `scope_skew` table, GAP rows 292 and 419, and a possible link between the omnibox history test (D2-006) and the same flush cause.

## Ledger notes for the lead

- D2-175 is fixed by `3e47e5725`. `DEPTH-FINDINGS-W2.md` is not edited here; the integration step can mark it.
- D2-037 is corrected for GAP rows 177 and 201 by `bec2e32d6`. Rows 292 and 419 still need the same fix.
- The D2-073 default is written up in `CRAWL_SCOPE_ROBOTS.md` section 13 (`d0a528e03`).

## Not done

- Nothing pushed, no PR, no `--no-verify`, no compile, no test run, no Docker work.
- The four line-ending-only `.sha256` files in `innh-lane-integ` were not touched; this unit stages only its own paths.
