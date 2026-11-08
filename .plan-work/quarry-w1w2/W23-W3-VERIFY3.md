# W23 W3 verify 3: CW-1 / CW-2 / CW-3 (2026-11-08)

Worktree `innh-lane-integ`, branch `w23/integ`, HEAD a5f692da2 (CW-1 9da2c64b8, CW-2 7d64ea5a5, CW-3 a5f692da2). Working tree clean, so the build is the commits alone. No writer files edited.
Build: `CARGO_TARGET_DIR=C:\qv2t-w3v2`, `-j 2`, one invocation: `cargo test -p quarry-runtime -p quarry-edge --lib -- screenshot redact_pii redacting delivered withheld otp_read crawl_refuses landing_back pure_read`. (`--lib` twice is rejected by cargo; pass it once.)

## Counts

| Binary | Passed | Failed |
|---|---|---|
| quarry-edge lib | 10 | 0 (1079 filtered) |
| quarry-runtime lib | 18 | 0 (1373 filtered) |
| Total | 28 | 0 |

## Per commit

- CW-1 (crawl refuses `redact_pii`): `crawl_scrape::tests::crawl_refuses_redact_pii_never_silently_ignores_it` passes. The three `routes::tests::redact_pii_*` scrape tests still pass (off by default, ZDR redaction, refused outside ZDR).
- CW-2 (redacting run refuses capture): `observation::tests::a_redacting_run_refuses_the_screenshot_and_stores_nothing` passes. `pipeline::pii_redaction_tests::a_redacting_run_keeps_emits_and_returns_no_email` passes.
- CW-3 (delivered one-time-code page withheld before store): all pass.
  - runtime: `a_delivered_page_stores_no_capture_and_announces_no_screenshot`, `a_pure_read_of_a_delivered_page_is_refused_before_the_browser_runs`, `a_pure_read_asks_the_browser_which_page_it_is_on`, `landing_back_on_a_delivered_page_is_withheld_and_landing_elsewhere_is_not`, and all `otp_read_denial_tests` including `a_page_is_keyed_by_the_address_its_observation_carries`.
  - edge: `a_delegate_page_read_of_a_delivered_page_is_refused_before_its_step_key`, `a_delegate_step_on_a_delivered_page_returns_its_observation_withheld`, `a_failed_fill_marks_the_page_and_a_screenshot_of_it_is_refused`, `the_edge_uses_one_process_wide_set_of_marks`.

This closes verify 2 blocker 2 (the OTP work is now committed and its tests pass at the commit).

## Still open (unchanged from verify 2)

1. CW-2 `applies(false, ..)` (observation.rs:698 and :730) ignores a per-run `redact_pii` request on the agent path; only the org default `QUARRY_EDGE__PII_REDACTION_DEFAULT_ORGS` triggers the refusal. Needs a decision: thread the flag or document it.
2. CW-2 Pdf arm has no test (only Screenshot).
3. Warning `unused import: TEST_PII_REDACTION` in the CW-2 test (observation.rs, line ~5120). Cosmetic. Other warnings in the build are pre-existing (`FleetStatus`, `MutexGuard`).
4. The Browserless token-in-URL item from verify 2 (`lab/evals/src/provider_matrix.rs:316`) was not re-checked here; it is not in scope of this targeted run.

## Not run

Full lanes, integration tests, the lab/evals crate, and any run with `QUARRY_EDGE__FILL_OTP` or `QUARRY_EDGE__PII_REDACTION` on in a live stack. The CW-3 unit tests exercise the flag through the `read_denial` parameter and marks directly.
