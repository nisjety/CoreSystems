# W2-U06 progress (WS12.S6, log hygiene lint)

Unit W2-U06, batch 1, worktree `C:/dev/CoresSystem/.claude/worktrees/vigorous-perlman-e7ae5b`, branch `quarryv2-next`. Build-first wave: tests written and compile-checked, not run.

## Built

- `quarry-core/src/error.rs`: `QuarryError::log_message()` (message through the WS12 Durable rules, no details, Display unchanged) with a private `log_message_under(mode)` for tests; tests in `mod log_message_tests` (`cargo test -p quarry-core error::log_message`, written, not run).
- `quarry-runtime/src/browser_driver.rs`: hydration settle log now formats the page URL through a local `log_url` helper (Durable URL rules). This was the only runtime call site the lint flagged that was not already exempt.
- `quarry-runtime/tests/log_hygiene.rs` (new): ratchet over `crates/*/src`, banned in tracing calls: `?action`, `?request`, `?observation`, `.text`, `.value`, `dom_summary`, `accessibility`, `url = %` unless wrapped in `redact_url(` or `log_url(`. Exempt: reasoned `zdr-log-ok:` comment on the macro line or the line above (WS4.F convention), or a `log_hygiene.allow` entry. Self-tests: `scanner_flags_each_banned_form`, `scanner_passes_wrapped_urls_and_closed_labels`, `scanner_skips_test_items_and_needs_a_reasoned_exemption`, `allow_file_entries_are_well_formed`; ratchet `production_logs_carry_no_page_or_caller_text`.
- `quarry-runtime/tests/log_hygiene.allow` (new): header only, empty list.
- `docs/RUNBOOKS.md`: section "Log hygiene lint and the log_message helper (WS12.S6, unit W2-U06)" appended.
- No flag, no migration, no contract or openapi change, no golden change.

## Plan sites that needed no edit

The plan's named sites (action_runtime.rs 174/178, observation.rs 829-835/899/920, page_extract.rs 459-491, answer.rs 781) already log `error_class` or closed labels in the current tree. Recorded as a doc item in depth/W2-U06.md.

## Verification so far

- `cargo check --tests -p quarry-core -p quarry-runtime` through qtest: rc 0, no error or warning lines in the filtered output (run before the final rustfmt pass).
- `rustfmt --edition 2021 --check` on `error.rs` and `log_hygiene.rs`: clean after formatting. `browser_driver.rs` has six pre-existing diffs in its test module (lines 679 and later), not in this unit's hunk.
- Node prototype of the banned-token rules over `crates/*/src` (a manual scan, not the Rust test): 4 hits before the browser_driver fix, 0 after the fix with zdr-log-ok honoured. The Rust test has not been run.

## Not done (next batch or final test phase)

- Run `cargo test -p quarry-core error::log_message` and `cargo test -p quarry-runtime --test log_hygiene` in the final test phase.
- Raw error text in tracing (`error = %e`) is not checked by this lint; see depth/W2-U06.md.
- RUNBOOKS.md carries another unit's uncommitted section (U01 "Request id intake"); only this unit's hunk is committed.

## For the next batch

- The lint's banned list is fixed by the plan; a later unit that adds a tracing call with `?request` or `url = %` must wrap it or add a reasoned zdr-log-ok comment, or the final test fails.
- `QuarryError::log_message()` is available for any site that must log error text.
