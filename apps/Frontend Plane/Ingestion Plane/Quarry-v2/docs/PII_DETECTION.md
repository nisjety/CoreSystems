# PII detection (E3.6-1)

Status: library only. `crates/quarry-transform/src/pii.rs` is a pure module
(no I/O, no logging, no environment reads). Nothing in Quarry calls it yet. The
redaction stage that sits between markdown derivation and every sink, the
per-request option and the org default are later slices (E3.6-4, E3.6-5, not
built). Default behaviour of Quarry is unchanged.

## What it detects

| Kind (`PiiKind`) | Marker written by `redact` | Candidate | Validator |
| --- | --- | --- | --- |
| `Email` | `[redacted:pii.email]` | ASCII local part, dotted domain, letters-only TLD | `is_email`: local part rules, label rules, TLD 2 to 24 letters |
| `Phone` | `[redacted:pii.phone]` | `+` form, or Norwegian 8 digits grouped 3-2-3 or 2-2-2-2 | `is_phone`: 8 to 15 digits after `+` (first not 0); national 8 digits starting 2 to 9 |
| `Iban` | `[redacted:pii.iban]` | Capital letters, two check digits, groups of 1 to 4 | `is_iban`: length for 16 known countries (15 to 34 otherwise), mod-97 = 1 |
| `PaymentCard` | `[redacted:pii.card]` | 13 to 19 digits with single spaces or dashes | `is_payment_card` = WS12's `quarry_core::redaction::luhn::is_pan` (issuer prefix, length, Luhn) |
| `Fodselsnummer` | `[redacted:pii.fodselsnummer]` | 11 digits, optional space after the date | `is_fodselsnummer`: D-number (+40 day) and H-number (+40 month) date fields, both mod-11 control digits, remainder 10 never valid |

Each candidate is kept only when its validator passes. A rejected candidate is
retried from the next character, so a valid number that starts inside a
rejected run is still found. Overlaps are resolved by the scan order above
(email first). `scan` returns findings sorted by start; `redact` replaces
them with the markers and copies everything else byte for byte.

## Entry points

- `pii::scan(text) -> Vec<PiiFinding>`: kind and byte range of each finding.
- `pii::redact(text) -> String`: the text with markers.
- `pii::is_email`, `is_phone`, `is_iban`, `is_payment_card`, `is_fodselsnummer`:
  the validators, usable on their own.

## Golden corpus

`crates/quarry-transform/tests/fixtures/pii/golden.json` (schema
`quarry-pii-golden/1`) holds validator verdicts and text cases with the exact
expected findings, in three groups: `norwegian`, `english` and `near_miss`.
`crates/quarry-transform/tests/pii_golden.rs` checks the validators, the scan
output, that redaction removes every expected match, and that the corpus covers
each kind with both verdicts. The numbers in the corpus are published examples
or were computed with the check they test.

## Commands

```bash
cd "apps/Ingestion Plane/Quarry-v2"
cargo test -p quarry-transform pii_golden   # validators, scan and redact against the corpus
cargo test -p quarry-transform pii_         # including the marker and remainder-10 tests
```

## Limits (read before wiring)

- Email local parts are ASCII only. Addresses with æøå are not detected.
- The national phone form has no context check: an 8-digit reference grouped
  3-2-3 that starts 2 to 9 is redacted. Expect over-redaction on reference
  numbers until a Norwegian public-sector fixture set exists (E3.6-10).
- Fodselsnummer checks date fields and control digits only. Individual-number
  ranges and calendar validity (for example 31 February) are not checked.
- The IBAN length table covers 16 countries. Others pass the mod-97 check at
  any length from 15 to 34.
- Nothing decides what a page does when redaction fails. The fail-closed
  semantics (a page whose redaction errors becomes a failed page with a typed
  reason, never empty markdown) are gated on a decision and belong to E3.6-4.
- The OCR and PII model tier (`ai_language_pii`) is not used here. It runs only
  under the model-lane decision (DR-15.1) and is never called under ZDR.

## Zero data retention

The module logs nothing and stores nothing. The wiring slice must keep this
true: the local PII stage runs under ZDR and persists nothing when it fails.
