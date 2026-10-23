# W2-U03 hand-over (WS8.A spike and ledger)

## What this unit did

WS8.A spike built to scope (throwaway, scratch only) and compile-checked; ledger entry drafted but not landed. Progress: `.plan-work/quarry-w1w2/W2-U03-progress.md`.

## Traps found

- `new_tab` always activates the new tab (`chromiumoxide.rs:1423`), and the trait doc (`crates/quarry-browser/src/lib.rs:607-608`) says so. The plan's "background tab" wording assumes a background open that does not exist.
- The plan's detail paragraph names four tests in "WS3.B's live lane". None exist in the tree (grep for `background_tab`, `pinned_session`, `per_tab_policy` finds nothing).
- Scratch worktree: `git worktree add --detach` under the scratchpad works. Remove it with `git worktree remove --force` followed by `git worktree prune`. The scratch copy checks out at HEAD, so it does not include other agents' uncommitted edits.
- `qtest.sh` run as a background `&` chain: the redirect only applied to the last command, and the log stayed empty. Run it in the foreground, or wrap the whole chain.
- Attribution: the repo conventions allow a per-commit Co-Authored-By line; this unit's commit uses the model name from the harness attribution reminder.

## Ledger entry draft (NOT landed: needs sibling (b) OK in `apps/Model Plane/docs/decisions/ledger.md`)

Title: Shared browser sessions: human_browse and attach grants, renewal by presence, per-tab control

Body (draft, to check against the plan at landing):

- Records D8-1 to D8-3, D8-5 and D8-7 of `docs/plans/quarry-browser-build-plan-2026-10-14.md`.
- Proto allocation: browser.proto numbers 50-59 belong to WS8 (`policy_mode`, `denied_domains`, `absolute_expires_at`, `resource_id`, `RenewGrant`). Confirmed unused in the Model Plane proto on 2026-10-23.
- Attach uses a service principal. The OD13 delegated bearer (`aud=quarry`) is rejected for attach. The Option-2 delegation claim (RFC 8693 `acting_user_id`) is deferred.
- Spike facts (code read at HEAD 2ac764f1; live measurements pending): new tabs activate on creation; actions reach only the active tab; egress policy is per session; page hygiene applies focus emulation to every page at attach.
- Open item recorded for WS8: the SPA sends `actor:'agent'` under the user's session (`KnowledgeComposer.tsx:1050`). The gateway does not check the caller class for agent calls. Not allowed as a pattern for attach.

If sibling (b) does not sign off, this text stays here and the hand-over is the only record.

## Measurements still owed (final test phase or stack lane)

1. Background throttle: `background_tab_is_not_throttled` (live, `CHROMIUMOXIDE_TEST=1`). Expected about 20 ticks in 2 s if not throttled.
2. Two-tab Chromium RSS: `measure_two_tab_rss` (`CHROMIUMOXIDE_TEST=1 CHROMIUMOXIDE_MEASURE=1`).
3. F3 `domain_grant_blocked` count, one-host policy versus open web (stack lane, egress receipts). Not run.
4. Broker validation latency at 20 actions/s (stack lane, Go broker). Not run. Decides whether `QUARRY_EDGE__GRANT_VALIDATION_CACHE_MS` stays 0.
5. The three active-tab and pin tests in the spike show the current behaviour, not a pass or fail. They become real tests only in WS8.E.

## Files touched

- Committed: `.plan-work/quarry-w1w2/W2-U03-progress.md`, `.plan-work/quarry-w1w2/wf-U03.md`, `.plan-work/quarry-w1w2/wf-U03-progress.md`, `.plan-work/quarry-w1w2/depth/U03.md`, `.plan-work/quarry-w1w2/spike/U03-spike-tests.rs.txt`, `.plan-work/quarry-w1w2/spike/U03-spike-chromiumoxide.patch`.
- Not touched: all product code, `apps/Model Plane/docs/decisions/ledger.md`, `docs/plans/*`.
