# W2-U05 progress (WS17.S0, CDP capability spike)

Status: BUILT, compile-checked, NOT run. Built to the slice's scope: a test-only spike, no product code.

## Committed (this unit)

- Test: `apps/Ingestion Plane/Quarry-v2/crates/quarry-browser/tests/profile_cdp_spike.rs` (new). Test `cdp_capability_spike`, `#[ignore]`, gated by `quarry_browser::live_gate::live_enabled()`. Probes P1 (a, b, c, latency P5), P2 (a, b), P3 (a, b, c), P4 (a, b, c). It prints a `probe | outcome` table and asserts only that every probe left a row.
- RUNBOOKS: a section "WS17.S0 CDP capability spike: run it and file the table" appended to `apps/Ingestion Plane/Quarry-v2/docs/RUNBOOKS.md`. Committed as a hunk only (see commit list in the final answer).
- Depth bullets: `.plan-work/quarry-w1w2/depth/W2-U05.md`.

## Verified (what was actually run)

- `bash qtest.sh 'cargo check -p quarry-browser --features chromiumoxide --tests'`: first run failed with one error (E0609, `EvaluateReturns` has no `value`; the field is `result.result.value`). Fixed. Second run: `Finished`, no errors, no warnings.
- `rustfmt --edition 2021 --check` on the new file: diffs found (line wrapping only). `rustfmt` applied to the file (rc 0). The cargo check after that passed.
- Not run: the spike itself (needs the live Chromium lane and outbound HTTPS to example.com). Not run: any other test.

## Not done

- The spike has not been run. There is no result table yet, so WS17.C1 must not fix its architecture on this spike yet.
- The plan's expected answers (P2 "no", P3 "yes") are not asserted; that waits for the first live run (depth bullet 2).
- The spike does not use the pinned egress proxy (depth bullet 3).

## For the next batch

- WS17.C1 (U55) starts after the table exists. It needs the table from `cdp_capability_spike` run with `CHROMIUMOXIDE_TEST=1 QUARRY_BROWSER_NO_SANDBOX=1 CHROME=/usr/bin/chromium cargo test -p quarry-browser --features chromiumoxide --test profile_cdp_spike -- --ignored --nocapture`, run in the final test phase.
- The RUNBOOKS hunk for U04 (WS15.S0) and U01 (WS10.S1) sections was uncommitted in the shared tree when U05 committed. Those units must commit their own hunks against HEAD as it stands; the U05 section sits at the end of the file in the working tree.
- Commit attribution: the commits carry `Co-Authored-By: Claude Haiku 5.5`, per the session's attribution rule. The workflow text asked for a different line; the session rule was followed (see the final answer).

## Commit list

- 5a990a667 test(browser): WS17.S0 CDP capability spike (the test file).
- 0720bb562 docs(quarry): RUNBOOKS section for the WS17.S0 CDP capability spike (hunk only).
- This notes commit (docs(plan-work)): progress, depth bullets, continuation pointer.
