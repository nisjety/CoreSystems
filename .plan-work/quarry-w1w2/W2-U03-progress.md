# W2-U03 progress (batch 1, slice WS8.A, spike and ledger)

Updated 2026-10-23. Hand-over and ledger text: `.plan-work/quarry-w1w2/wf-U03.md`. Depth bullets: `.plan-work/quarry-w1w2/depth/U03.md`.

## Scope reminder

WS8.A has no product code. The spike is a throwaway `cfg(test)` branch in a scratch worktree and is never merged. The only merge candidate is the ledger entry in `apps/Model Plane/docs/decisions/ledger.md`, which needs sibling (b)'s OK.

## Committed

- Plan-work files only (see the commit line in the final report).

## Built (in scratch, not merged)

- Spike tests added to a scratch copy of `crates/quarry-browser/src/chromiumoxide.rs` inside `mod tests`: `background_tab_does_not_change_active_tab`, `pinned_session_acts_on_pinned_tab_while_other_tab_is_active`, `per_tab_policy_blocks_in_agent_tab_only`, `background_tab_is_not_throttled`, `measure_two_tab_rss`. All are `#[ignore]`, gated on `CHROMIUMOXIDE_TEST=1` (and `CHROMIUMOXIDE_MEASURE=1` for the RSS one). Each prints a `SPIKE ...` line on stderr and asserts nothing.
- Saved for WS8.E: `.plan-work/quarry-w1w2/spike/U03-spike-tests.rs.txt` (the test block) and `.plan-work/quarry-w1w2/spike/U03-spike-chromiumoxide.patch` (the full scratch diff, 170 insertions, one file).
- The scratch worktree was removed after the compile check.

## Verified (commands actually run)

1. `qtest.sh 'cargo --version'` (smoke): cargo 1.99.1, rc=0.
2. `qtest.sh` with QROOT = scratch copy of Quarry-v2 at HEAD 2ac764f1 plus the spike insertion: `cargo check --tests -p quarry-browser --features chromiumoxide`: `Finished dev profile`, rc=0, no error lines.
3. Grep: proto field numbers 50-59 are unused in `apps/Model Plane/proto/model_plane/v1/browser.proto` and in the vendored `crates/quarry-runtime/proto/browser.proto` (the vendored file uses 40-44 only). Matches the plan's R-6 allocation.
4. Grep proof for "no unconfirmed caller sends actor:'agent' under the user token": one caller found, see below.

Not run: the five spike tests (live Chromium lane, deferred to the final test phase by the build-first rule). No stack measurements.

## Grep proof result (static, not run)

- `apps/Frontend Plane/verevonv3/src/features/dashboard/home/KnowledgeComposer.tsx:1050` calls `runBrowserAction(id, sessionId, action, { actor: 'agent' })` from `performBrowserSuggestedAction`. That call uses the user's org headers and session (`apps/Frontend Plane/verevonv3/src/shared/api/browser-client.ts:391-402`).
- The gateway takes `body.actor` as is (`apps/Frontend Plane/verevonv3/apps/gateway/src/domains/browser.rs:734`). It refuses an agent call only while takeover is active (763). It does not check that an agent-labelled call comes from a service principal.
- The other `actor: 'agent'` site is a Quarry-side constant, not a caller under the user token.
- Consequence: the agent-under-user-token path is live in the SPA today. WS8.F and WS8.E must not route attach through it, and the ledger should say so.

## Spike findings (code read, not measured)

- `open_tab_page` sets `active_tab_id` to the new tab (`apps/Ingestion Plane/Quarry-v2/crates/quarry-browser/src/chromiumoxide.rs:1423`). `new_tab` therefore always activates. A background open does not exist; WS8.E must add one.
- Actions address only the active tab (`active_tab_page` 1162-1181, `current_tab_page` 1183). `select_tab` (3885) is the only tab addressing. A pin needs a tab-addressed call path.
- The egress policy is per session: `configure_egress_policy` takes a session (3122-3139), and the network guard looks it up by session key (around 1665). A per-tab policy is not expressible today.
- `apply_page_hygiene` (3032) sets `SetFocusEmulationEnabled` and lifecycle `Active` on each page at attach (called at 1331). Background throttling may already be mitigated, but this is unmeasured.
- The four named WS3.B tests (`background_tab_does_not_change_active_tab` etc.) do not exist in the tree. The plan's note that they are in "WS3.B's live lane" is wrong for the current code.

## Not done (see wf-U03.md for the reasons)

- Ledger entry: not landed in `apps/Model Plane/docs/decisions/ledger.md` (no sibling (b) OK). Draft text is in wf-U03.md.
- Measurements: F3 `domain_grant_blocked` count (one-host versus open web), broker validation latency at 20 actions/s, two-tab Chromium RSS, background throttle result. All need the live lane or the stack and are deferred.
- `QUARRY_EDGE__GRANT_VALIDATION_CACHE_MS` default: depends on the latency measurement.
- `docs/BROWSER_WORKSPACE_PLAN.md` correction: belongs to WS8.F.

## Next batch needs to know

- WS8.B and WS8.E can start from the findings above. WS8.E must add a background open, a tab-addressed call path and tab-scoped egress, none of which exist.
- Do not use `actor:'agent'` from the SPA under the user's token as a pattern. Route agent attach through a service principal (OD13 rejected).
- Re-run the spike snippet in the final test phase (`CHROMIUMOXIDE_TEST=1`) before WS8.E relies on any of its findings.
