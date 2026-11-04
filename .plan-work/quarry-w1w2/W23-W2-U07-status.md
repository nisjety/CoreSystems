# W2-U07 status: open_tab keeps its tab when the first navigation times out

Branch w23/lane-a. Date 2026-11-03. Code only: not compiled, no tests run (owner order for W2/W3 unit work).

## Built
- `open_tab` with a URL (the attach-time navigation, `FirstNavigation::InAttach`): a navigation TIMEOUT no longer closes the tab. The abandoned navigation is stopped before the tab is registered, so the tab is readable. The tab is registered and marked in the same step, so its next `goto` drains the navigation (SC4 T3). The caller gets the navigate TIMEOUT with `details.tab_id`. The tab stays listed, and it is active when `activate` is true.
- Every other navigation failure (refused URL, transport or DNS failure, Fetch-guard refusal) still closes the tab through the attach guard, as before.
- `tab_control.rs`: pure rules `keeps_tab_on_navigation_failure(code)` (true only for Timeout) and `timed_out_open_error(tab_id, error)`. The second adds `tab_id` and keeps the code, message, `op`, `deadline_ms`, `effect` and `retryable`. It never adds the URL (ZDR).
- Already existed, not rebuilt: `goto` on a session's first tab commits `about:blank` and navigates with the TIMEOUT recovery (SC4 T4b), and the next `goto` drains the abandoned navigation (SC4 T3). Only the `open_tab` attach path was missing this. Corrected the stale `FirstNavigation::InAttach` doc ("answers with the tab it made").

## Commits (local on w23/lane-a, not pushed)
- ce057a9c3 fix(quarry-browser): helpers for an open_tab that keeps its tab on a navigation timeout (W2-U07), `tab_control.rs`
- 5983ca925 fix(quarry-browser): open_tab keeps its tab when the first navigation times out (W2-U07), `chromiumoxide.rs`
- Third commit: this status file and the lane-a depth notes (see the commit log).

## Files
- apps/Ingestion Plane/Quarry-v2/crates/quarry-browser/src/tab_control.rs
- apps/Ingestion Plane/Quarry-v2/crates/quarry-browser/src/chromiumoxide.rs
- .plan-work/quarry-w1w2/W23-W2-U07-status.md (this file)
- .plan-work/quarry-w1w2/W23-depth-lane-a.md (depth notes appended)

## Tests written (not run)
- `tab_control::tests`: `only_a_navigation_timeout_keeps_the_tab`, `the_timed_out_open_names_the_tab_and_keeps_the_navigate_fields`, `a_timeout_without_details_still_names_the_tab`, `a_non_object_timeout_detail_is_replaced_by_the_tab_id`.
- `chromiumoxide::tests::a_timed_out_open_tab_keeps_its_tab_and_names_it` (`#[ignore]`; needs `CHROMIUMOXIDE_TEST=1` and Chromium; about 25 s; needs `example.com` to resolve publicly, because the URL guard looks the host up). It checks: a TIMEOUT with `details.tab_id` and no URL; the tab listed and active; one abandoned navigation marked; the next `goto` drains it and answers within 8 s.
- Mock-driver unit tests: not built as a CDP-free double. No double exists for the attach path, since `open_tab` needs a launched Chromium page. The decision and the error shape are therefore pure helpers in `tab_control.rs`. The attach behaviour itself is covered only by the `#[ignore]` live test.

## Flag and default
No flag (bug fix).

## Assumed decisions
- Scope read as TIMEOUT only. A SecurityBlocked first navigation from `open_tab` still closes the tab, while `goto` keeps its tab on a refusal. Parity is a decision for the security owner (depth D-U07-2).
- The error keeps the navigate timeout's code, message, fields and `retryable: false`. It adds `details.tab_id` and nothing else. No URL in it (ZDR).
- The stop runs before registration, because the tab is invisible to other callers until then. The abandoned mark goes in with the registration, under the pages lock, so no `goto` can see the tab unmarked. The metadata refresh runs only if the stop succeeded (`goto`'s rule). The popup bridge is not installed on a timed-out document (`goto`'s rule).
- The live test uses the shipped 25 s navigate deadline and the fixture proxy with an allowed `example.com` host, not a test-only seam. `goto_detour` covers `goto` only.

## Model Plane asks
None. No file under apps/Model Plane was touched, and MP-ASKS.md was not appended.

## Not verified
- Not compiled, and no tests were run (code only). Parse check only: `rustfmt --check` on copies of both files reports zero parse errors. The new code adds no formatting diffs (the remaining diffs are pre-existing).
- Live behaviour not run (no Chromium run here).
- Callers outside this unit's files: quarry-edge `agent_routes/tabs.rs` (`new_tab`) and `agent_routes/attach.rs` (`open_tab`) now receive a TIMEOUT for a kept tab, with `details.tab_id`, but they neither record nor close that tab. See depth D-U07-1 and SR-U07-1.
