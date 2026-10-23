# WS12.S0 design note (unit S-WS12.A)

Slice: rule module `quarry_core::redaction`, golden `secret-shapes.v1.json`, conformance harness, sync rows. Pure library, nothing wired, no flag, no deploy.

## Data model
- `Profile {Live, Durable}`, `Mode {Enforce, Shadow, Off}` (process-wide, `init_mode` sets it once, the first `mode()` read pins the default `Enforce`, the library never reads env), `MarkerKind` (password|otp|card|phone|secret|token|key|credential) with `mask(kind)` = `[redacted:<kind>]`.
- `shapes.rs` holds one rule table `{id, kind, min_profile, regex, replace(Whole|Groups), verify}` compiled once in an `OnceLock`, plus a `RegexSet` per profile as the pre-pass. Rules: the Live shapes (pem, jwt, userinfo, aws, github, gitlab, npm, slack, google, openai, stripe, whsec, bearer, basic) and the Durable contextual rules (otp forward/reverse, url fragment/query, labelled secret) plus the Luhn PAN runs from `luhn.rs`.
- `field.rs`: `FieldFacts` (custom `Debug` that never prints the value), `classify` returning kind plus the rule id (field.password/otp/card/phone/label/luhn), `mask`.
- `url.rs` `redact_url` (string-level, keeps scheme, host, path and key names), `json.rs` `redact_json` (leaves through the text rules; Durable adds the object-key rule), `stamp.rs` `RedactionStamp` (counts by static rule id only).
- `Tally` is the only accumulator: counts per static rule id, no content, no lengths.

## Failure cases handled
- Idempotency: the scan loops to a fixed point (the loop ends when no rule replaced anything, cap 4); markers are skipped by every rule (`[redacted:` prefix check), so `redact(redact(x)) == redact(x)`; proptest over 2,000 strings on both profiles.
- False positives: bearer and basic need either an `Authorization` context or a token that is long and has a digit or base64 symbol (so "bearer of the message" and "Basic information" survive); OTP labels exclude bare `code` and `pin`; PAN runs need an issuer prefix, a Luhn pass, no adjacent letters, and card-like group sizes (UUIDs, hashes and order numbers survive).
- Unterminated PEM is redacted to the end of the text (fail closed).
- Rule drift: `rule_fingerprint()` hashes every pattern, id and key list; a test pins it against `RULE_VERSION`, so a rule change without a version bump fails.
- Regex engine is `regex` (linear time); perf budget test (release, ignored) 1 MiB under 150 ms.

## ZDR consequence
No I/O, no `tracing`, no persistence. Nothing stored: the stamp holds counts and static rule ids only. Live applies under ZDR as well (Durable is a superset). Shadow mode counts but returns the input unchanged, so it is for staging only; `Mode::permitted_in_production` lets the edge (S2) enforce "production accepts only Enforce".

## Flag and default
No flag (library only, nothing calls it). Default mode `Enforce`. Rollback is a revert.

## Deliberate deviations (reported)
- The plan's "JSON leaves capped at 64 KiB" is not implemented as truncation or skipping: truncating would break extraction results and skipping would leak. Leaves of any size are scanned whole in linear time and the 1 MiB perf test is the guard.
- Two extra rule ids beyond the slice list: `shape.whsec` (named in the table row) and `json.key` (the object-key rule).
- The object-key rule runs in Durable only (Live would blank legitimate extraction fields such as `password_requirements`).

## What I will not do
Wire anything (S1, S2, S3, S5, S6 stay open), create `mp-scrub` or vendor the golden into the Model Plane (S4), set `REQUIRE_CONTRACT`, edit `.gitleaks.toml`, remove `redact_sensitive` (it is documented as superseded), touch Model Plane files, or commit another session's WS5.G hunks.
