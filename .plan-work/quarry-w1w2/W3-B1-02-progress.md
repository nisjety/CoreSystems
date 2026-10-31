# W3-B1-02 progress (E3.6-1: quarry-transform pii.rs, validators and golden corpus)

Status: BUILT AND COMMITTED. Compile-checked only; tests written, not run (build-first rule).

## Committed (branch quarryv2-next, local, not pushed)

- cb2511b95 feat(quarry-transform): PII validators and scanner, not wired (E3.6-1)
  - `crates/quarry-transform/src/pii.rs` NEW: PiiKind (email, phone, iban, card, fodselsnummer), validators is_email, is_phone, is_iban, is_payment_card (reuses quarry-core `redaction::luhn::is_pan`), is_fodselsnummer; scan(), redact(); markers `[redacted:pii.<kind>]`.
  - `crates/quarry-transform/src/lib.rs`: one mod line (`pub mod pii;`).
- 8b873e592 test(quarry-transform): PII golden corpus and validator tests (E3.6-1)
  - `crates/quarry-transform/tests/pii_golden.rs` NEW (tests pii_golden_*, pii_marker_is_stable_per_kind, pii_fodselsnummer_rejects_a_remainder_of_ten).
  - `crates/quarry-transform/tests/fixtures/pii/golden.json` NEW (schema quarry-pii-golden/1; 37 validator cases, 14 text cases in norwegian, english, near_miss groups).
  - `docs/PII_DETECTION.md` NEW (slice doc; used instead of a RUNBOOKS.md section because RUNBOOKS.md carries another agent's uncommitted hunks).

## Verification run (what was actually run)

- `cargo check --tests -p quarry-transform` via qtest: first run Finished clean (2.19 s, no errors, no warnings). Re-run after rustfmt: no error or warning lines.
- `rustfmt --edition 2021 --check src/pii.rs tests/pii_golden.rs`: first run showed formatting-only diffs; applied rustfmt in place to the two new files; re-check exit 0.
- gitleaks pre-commit hook (via qcommit.sh): no leaks found on both commits.
- NOT run: `cargo test -p quarry-transform pii_` (test suite, deliberately not run per the build-first rule). The hand-computed expectations in golden.json are not yet proven by a run.

## Worked-out values used in the corpus

- Fodselsnummer checksums computed with a node script (mod-11, same rule as the Rust code): 12058512336, 13058512375, 12458512319 (H), 52058512400 (D), 01010078950, 31129900183, 12054512323 valid; 010190456 has remainder 10 (no valid control digit).
- IBAN mod-97 checked with node: NO9386011117947, GB82 WEST 1234 5698 7654 32, DE89 3704 0044 0532 0130 00, SE4550000000058398257466 valid; NO9386011117948 invalid.

## Not done (out of this slice, by plan)

- The redaction stage and fail-closed semantics (E3.6-4; decision gated).
- Per-request option and org default (E3.6-5), the Norwegian 10-document fixture set (E3.6-10), the model tier (ai_language_pii, DR-15.1), any sink wiring. Nothing in Quarry calls pii.rs.

## Uncommitted files of this unit

None. The working tree for quarry-transform and docs/PII_DETECTION.md is clean after the two commits.

## Next

- End of wave: run `cargo test -p quarry-transform pii_` and fix any corpus expectation that the run disproves (most likely candidates: the IBAN trailing-word trim case and the invalid-card-then-valid-card case, both depend on the regex retry behaviour).
- Depth bullets: `.plan-work/quarry-w1w2/depth/W3-B1-02.md`. Hand-over notes: `.plan-work/quarry-w1w2/wf-W3-B1-02.md`.
