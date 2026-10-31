# W2-U17 progress (WS12.S1, field-facts probe and AX/target masking)

Status: BUILT to scope and compile-checked. NOT run: no test suite, no Chromium run, no Q10 run. Nothing is deployed. The flag is on by default; no caller outside the browser projection reads the new fields.

## Committed

- `1f829687c` feat(quarry-browser): field-facts probe and AX value masking (WS12.S1)
  - quarry-browser: `src/field_probe.rs` NEW (pure masking: `collect_probe_targets`, `facts_from_attributes`, `facts_from_call_result`, `mask_projection`, `probe_setting`; cfg-gated probe: `run_probe`, `describe_facts`, `call_facts`; 400 ms budget, 64-node cap, concurrent via FuturesUnordered); `src/field_probe_live.rs` NEW (live test `ax_field_probe_live`, included in the chromiumoxide test module); `lib.rs` (+18: `mod field_probe`, `BrowserNativeProjection.redaction`, `ProjectionRedaction`, `BrowserNativeTarget.field_kind` with serde skip); `chromiumoxide.rs` (only the WS12.S1 hunks: `native_page_projection` probes and masks, two literals, the include); `trusted_input_live.rs` (+1 literal).
  - quarry-runtime: `src/test_site.rs` (+20) serves `GET /secrets-lab`.
  - `scripts/q10/ax-password-probe.mjs` and `scripts/q10/secrets-lab.html` NEW (Q10 recipe and fixture page; not run).
- `ea229444a` docs(quarry-v2): WS12.S1 secret-redaction and runbook sections
  - `docs/SECRET_REDACTION.md` (+14): S1 section. `docs/RUNBOOKS.md` (+11, hunk-only): S1 runbook section.

## Not committed (other units' uncommitted work, left in the working tree)

- `chromiumoxide.rs` WS17.P1 hunks (profile capture near line 1656, SaveMeta test near line 7036).
- `kernel.rs`, `session.rs`, `policy.rs`, `contracts.rs`, `quarry-transform/parse_cache.rs`: not ours.
- `docs/RUNBOOKS.md`: another unit's hunk (a WS17.S0 section removal in the working copy).

## Verified (what was actually run)

- `cargo check -p quarry-browser --features chromiumoxide --tests` (qtest, quarry-rust-env:2): rc 0, no warnings. This includes the live test file (compiled, not run).
- `cargo check -p quarry-browser --tests` (qtest): rc 0, no warnings.
- `cargo check -p quarry-runtime --tests` (qtest): rc 0.
- `rustfmt --edition 2021 --check` on `field_probe.rs` and `field_probe_live.rs` (qtest): clean after one `rustfmt` run that formatted those two new files only.
- `git apply --check --cached` on both patches: applied; `qcommit-patch.sh` gitleaks scan: no leaks, both commits.
- Earlier failures that were not ours, now resolved by the tree moving on: `SaveMeta` unresolved in kernel.rs and chromiumoxide.rs (another unit mid-edit); `ParseCacheKey` lacks Debug in quarry-transform (another unit mid-edit). The final checks above ran after both were gone.

## Not run (build-first rule)

- The 13 pure unit tests in `field_probe.rs` (`cargo test -p quarry-browser field_probe`). Not run.
- `ax_field_probe_live` (`CHROMIUMOXIDE_TEST=1 QUARRY_BROWSER_NO_SANDBOX=1 CHROME=/usr/bin/chromium cargo test -p quarry-browser --features chromiumoxide ax_field_probe_live -- --ignored --nocapture`). Not run.
- Q10 (`scripts/q10/ax-password-probe.mjs` against the pinned Chromium) and the fixture `tests/fixtures/ax-password-probe.json`. Not produced.
- The `/secrets-lab` route: compiled, not exercised.
- Probe cost (`probe_ms`): unmeasured.

## Not done (in scope, not finished)

- Q10 answer: describe-first versus resolve path, and whether a password AX value is plain, bullets or absent. Both paths are implemented; the evidence decides which stays.
- Observation-level stamp, `RedactionStamp` conversion and the fingerprint inheritance test: WS12.S2.
- Typed-text refusal for Password and Otp field kinds: WS12.S3.

## For the next batch

- WS12.S2 builds the observation stamp from `ProjectionRedaction` (`probe`, `probe_ms`, `values_masked`, `by_rule`). Decide the counting rule there (AX nodes are counted, targets are not).
- `BrowserNativeTarget.field_kind` is set on editable targets and is consumed by nothing yet.
- The Q10 gate: the plan says Q10 runs before S1 merges. This commit is on the branch but not merged; the test phase must run Q10 first.
