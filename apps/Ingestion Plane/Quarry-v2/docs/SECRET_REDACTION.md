# Secret redaction at capture (WS12)

One deterministic, versioned rule module, `quarry_core::redaction`
(`crates/quarry-core/src/redaction/`), decides what a password, an OTP, a card
number or a token looks like and replaces it with a marker. It exists so that
one set of rules is applied before anything Quarry captures reaches a caller, an
artifact, an event, a receipt, a log or a model prompt, instead of each of those
places growing its own filter.

Rule set: `RULE_VERSION = "qr-redact/3"` (see "Version history"). Marker: `[redacted:<kind>]` with
`<kind>` one of `password`, `otp`, `card`, `phone`, `secret`, `token`, `key`,
`credential`.

## Status

WS12.S0 (this document, the library, the golden, the harness, the sync rows) is
the only slice built. **Nothing calls the module yet.** No runtime path masks
anything, no flag exists, nothing is deployed, and behaviour is unchanged.

| Slice | Wires the rules into | State |
|---|---|---|
| S0 | nothing: the library, the golden `secret-shapes.v1.json`, the harness, the sync rows | built |
| S1 | the driver: field-facts probe, accessibility-tree and target masking (`quarry-browser`) | open |
| S2 | observations, events, error text, the rule-version stamp, mode boot (`quarry-runtime`, `quarry-edge`) | open |
| S3 | receipts, typed text, checkpoints, procedures, read-time masking, migration `0013` | open (needs the X-19 receipt-policy answer) |
| S4 | the Model Plane `mp-scrub` crate, planner-prompt and event scrub, vendored golden | open (Model Plane) |
| S5 | screenshots, by element rectangle, fail closed | open |
| S6 | logs: nothing page-derived reaches `tracing`, a lint test | open |
| S7 | optional: scrub legacy durable rows (`0014`) | open (product decision) |

`redact_sensitive` (`quarry-runtime/src/agent_memory.rs`, re-exported from
`quarry-runtime/src/lib.rs`) is **superseded** by this module. It is not
removed by S0: nothing outside `agent_memory.rs` calls it (grep over the
Quarry-v2 workspace), and it is left in place until the call sites it would have
served are wired to `quarry_core::redaction`. Do not reuse it as a seed: its
24-character token rule redacts ids and hashes, and its email rule is PII
handling, which is out of scope (F-t2-050).

## Profiles and modes

| Profile | Applied to | Rules |
|---|---|---|
| `Live` | everything Quarry returns to any caller, ZDR runs included | field-semantic values and high-confidence token shapes |
| `Durable` | everything kept or emitted beyond the caller: receipts, checkpoints, events, JSON artifacts, logs, error text | `Live` plus contextual rules (labelled secret assignment, numeric OTP beside an OTP label, Luhn-valid card numbers), URL query and fragment secrets, and the JSON object-key rule |

`Durable` is a superset of `Live`. An extraction result gets `Live` only, so
extraction keeps working.

| Mode | Effect |
|---|---|
| `Enforce` (default) | matches are replaced |
| `Shadow` | matches are counted, the input is returned unchanged (staging, to measure false positives) |
| `Off` | nothing happens |

The mode is process-wide and set once at boot by the edge (`init_mode`); the
first read pins the default, so a process never changes mode while serving. The
library never reads the environment. `ENVIRONMENT=production` accepts only
`Enforce` (`Mode::permitted_in_production`); the edge enforces that in S2.

## API

| Item | Purpose |
|---|---|
| `redact_text(input, profile, &mut Tally) -> Cow<str>` | free text |
| `redact_json(&mut Value, profile, &mut Tally)` | every string leaf, plus (Durable) object keys that name a secret |
| `redact_url(input, profile, &mut Tally) -> Cow<str>` | keeps scheme, host, path and key names; masks userinfo passwords and (Durable) secret query and fragment values; a non-URL goes through `redact_text` |
| `field::classify(&FieldFacts) -> Classification`, `field::mask` | a form field's kind from its type, autocomplete, labels and value |
| `Tally`, `RedactionStamp` | counts per static rule id; the stamp carries no content and no lengths |
| `mask(MarkerKind)`, `is_marker`, `RULE_VERSION`, `rule_fingerprint()` | marker text, version, drift digest |

Every function has a `_with(mode, ...)` variant for tests and for callers that
hold their own mode. The scan loops to a fixed point and every rule leaves text
that starts with `[redacted:` alone, so `redact(redact(x)) == redact(x)` and a
second pass counts nothing. The engine is `regex` (linear time); a `RegexSet`
pre-pass means text with nothing to mask costs one scan.

## Rules

Token shapes (both profiles):

| Id | Matches | Marker |
|---|---|---|
| `shape.pem` | a `PRIVATE KEY` block; an unterminated block is masked to the end of the text | `key` |
| `shape.jwt` | three dot-separated base64url segments starting `eyJ` | `token` |
| `shape.userinfo` | the password in `scheme://user:SECRET@host` | `credential` |
| `shape.aws` | `AKIA` / `ASIA` access key ids | `key` |
| `shape.github` | `ghp_ gho_ ghu_ ghs_ ghr_`, `github_pat_` | `token` |
| `shape.gitlab` | `glpat-` | `token` |
| `shape.npm` | `npm_` | `token` |
| `shape.slack` | `xox[baprso]-`, `xapp-` | `token` |
| `shape.google` | `AIza` API keys | `key` |
| `shape.openai` | `sk-` (OpenAI, Anthropic style) | `key` |
| `shape.stripe` | `sk_live_ sk_test_ rk_live_ rk_test_` (a publishable `pk_` key is public and left alone) | `key` |
| `shape.whsec` | `whsec_` webhook secrets | `secret` |
| `shape.bearer` | `Bearer <token>` with `Authorization` within 48 bytes before it in the same string, or a token of 16+ characters with a digit or base64 symbol (a long bare word of letters is prose and survives). A header dumped as JSON is covered by the `json.key` header rule below, because the header name is not in the string leaf | `token` |
| `shape.basic` | `Basic <base64>` after an `Authorization` header, or 12+ characters with a digit, `=`, `+` or `/` | `credential` |

Contextual rules (`Durable` only):

| Id | Matches | Marker |
|---|---|---|
| `ctx.labelled_secret` | `password`, `passwd`, `passord`, `secret`, `api_key`, `access_key`, `private_key`, `auth_token`, `client_secret`, `token` (with any `A_Z_` prefix) followed by `:` or `=` and a value, quoted or not; a bare value that is a state word (`null`, `required`, ...) is kept | `secret` |
| `ctx.otp_numeric` | 4 to 8 digits beside an OTP label, forward ("your verification code is 482913") or reverse ("482913 is your verification code"). Labels: `otp`, `totp`, one-time code/password, verification/confirmation/security/login code, passcode, 2fa code, `engangskode`, `engangspassord`, `bekreftelseskode`, `sikkerhetskode`, `verifiseringskode`. Bare `code` and `pin` are not labels | `otp` |
| `ctx.pan` | 13 to 19 digits with a known issuer prefix, an issuer-valid length and a passing Luhn sum, written as one run or as 3 to 6 digit groups joined by single spaces or dashes, and not glued to a letter | `card` |
| `url.query` | the value of `token`, `access_token`, `id_token`, `refresh_token`, `code`, `key`, `api_key`, `apikey`, `sig`, `signature`, `password`, `secret`, `session`, `sid`, `otp`, `jwt`, `x-amz-*` after `?`, `&` or `;` | `token` |
| `url.fragment` | the value of `access_token`, `id_token`, `refresh_token`, `token`, `code` in a `#fragment` | `token` |
| `json.key` (header rule, **both profiles**) | the value under an object key naming a credential header (any case): `Authorization`, `Proxy-Authorization` (`credential`), `Cookie`, `Set-Cookie` (`secret`), `X-Api-Key` (`key`), `X-Auth-Token` (`token`); and the `value` of a `{"name": "Cookie", "value": ...}` or `{"key": ...}` pair as HAR and CDP write headers. The whole value is replaced, scheme included, **whatever its shape**: an object or array under one of these keys is masked as a unit (over-masking by design: the key is the only context the walk has and a credential can sit in any child; a non-header field that happens to be called `authorization` loses its content) | per header |
| `json.key` | the value under an object key that names a secret (`password`, `credential`, `api_key`, `private_key`, `token`, `accessToken`, `secret`, `session_id`, `cookie`, env-style `*_KEY`). Counters (`max_tokens`, `token_count`) and booleans survive; a number, array or object under such a key is replaced whole | per key |

Field rules (`field.*`, the first hit wins): `type=password` is a password and
`type=tel` a phone (`field.password`, `field.phone`); checkboxes, buttons and
hidden inputs hold no typed value; autocomplete tokens after dropping
`section-*`, `shipping`, `billing` and contact words (`current-password`,
`new-password`, `one-time-code`, `cc-*`, `tel*`); a label regex over the
accessible name, `aria-label`, placeholder, `name` and `id`, camelCase and
punctuation normalized, English and Norwegian (`passord`, `engangskode`,
`bankid`, `kortnummer`, `fødselsnummer`, `personnummer`, `kontonummer`)
(`field.label`); and a value that is a Luhn-valid card number (`field.luhn`). A
username in a password form stays unclassified.

Two rule ids go beyond the list in the plan: `shape.whsec` (named in the slice
table) and `json.key` (the object-key rule).

## The golden and the harness

`crates/quarry-core/tests/fixtures/redaction/secret-shapes.v1.json` is
**authored**, not generated, and every secret in it is an array of fragments
that the loader joins, so no secret-shaped literal sits in the repository (and
`.gitleaks.toml` is not edited). It holds 36 positive cases (18 `live`, 18
`durable`), 27 negative cases (a 16-digit order number failing Luhn, a UUID, a
40-hex sha, a 64-hex hash, "pin the tab", a byte-identical `status=completed`
line, ...), 7 JSON cases and 30 field cases. The positive, negative and JSON cases list `applies_to`
(`quarry`, `mp`); the field cases do not (they are `quarry` only). `mp` is the set
the Model Plane scrubber will run: the `live` shapes and the negatives that guard
its over-redaction.

`crates/quarry-core/tests/redaction_golden.rs` is the loader. It runs every
case on both profiles (a `durable` case must be left alone by `Live`), checks the
`.sha256` pin, checks that every rule id is exercised by at least one case, that
no secret appears joined in the file, a property test that redaction is
idempotent over 2,000 generated strings (and URLs), the rule-drift test, the
mode and stamp tests, and a release-only performance budget (1 MiB of mixed
text under 150 ms). The budget test is `#[ignore]`, so `cargo test --workspace`
skips it; the `Redaction performance budget (release)` step of
`.github/workflows/quarry-v2-ci.yml` runs it with `--ignored`.

```text
cd "apps/Ingestion Plane/Quarry-v2"
cargo test -p quarry-core redaction
cargo test -p quarry-core --release redaction_perf_budget -- --ignored --nocapture
bash scripts/tests/zdr-contract-sync.test.sh            # from the repository root
bash scripts/tests/zdr-contract-sync.test.sh --self-test
```

`scripts/tests/zdr-contract-sync.test.sh` pins the golden's digest and lists the
Model Plane consumer `mp-scrub` as **pending** until WS12.S4 creates the crate
and vendors its copy. `REQUIRE_CONTRACT` stays unset in
`.github/workflows/contract-goldens.yml`.

## Changing a rule

Any change to a pattern, a key list, a field label, a threshold, the card
table, or what the rules do to the fingerprint's probe corpus (the bearer and
basic verify rules, the card group-size rule, the JSON header pairs) changes
`rule_fingerprint()`, and `redaction_rule_drift_requires_a_version_bump`
fails until you:

1. bump `RULE_VERSION` (for example `qr-redact/4`);
2. add the new `(version, fingerprint)` pair to `PINNED_FINGERPRINTS` in
   `tests/redaction_golden.rs` (keep the old pair out: a version names one rule set);
3. edit the golden, set its `rule_version`, refresh the `.sha256` pin beside it and
   the digest in `scripts/tests/zdr-contract-sync.test.sh`;
4. re-vendor every copy of the golden (the Model Plane `mp-scrub` crate, once it exists).

Add a negative case for every new rule: the label regexes are kept narrow on
purpose. The fingerprint hashes the data tables and the probe corpus's outcomes,
not the code: a behaviour change that no probe reaches is caught only by a golden
case, so give every verify rule and every threshold a positive and a negative case.

## Version history

- `qr-redact/1`: first rule set (WS12.S0, commit d405aa87). Never consumed: nothing
  calls the module and no copy of the golden was vendored.
- `qr-redact/2` (never consumed either): `Authorization` and `Proxy-Authorization` JSON object keys and
  `{name, value}` header pairs are masked under both profiles (a header dumped as
  JSON was not masked when its token was letters only), the drift fingerprint
  covers the verify and group-size behaviour, and the golden gained negatives
  for a bare letters-only `Bearer` word and for 7- and 2-digit groups in a card-like
  run.
- `qr-redact/3`: the header rule also covers `Cookie`, `Set-Cookie`, `X-Api-Key`
  and `X-Auth-Token` object keys and `{name, value}` pairs under both profiles
  (under `Live` they were only caught by the shape rules), and the golden gained
  three JSON cases (a cookie and api-key pair, cookie and api-key keys, a structured
  `authorization` value masked whole). Review residual of `qr-redact/2`.

## Honest limits

- **Not wired.** Until S1 to S6 land, Quarry masks nothing with this module.
- **[RISK] A page-displayed secret can still reach a scrape artifact** until
  F-t2-050 (PII) is decided: the rules recognise token shapes, labelled
  assignments and fields the driver can classify; a secret that is just text on a
  page, with no shape and no label, is not recognisable. `Live` is applied to
  extraction results precisely so that extraction keeps working.
- **Not PII.** Emails, names and addresses are not masked (F-t2-050).
- **Heuristic.** A password that follows an unquoted label and contains spaces is
  masked only up to the first space; `Basic <base64>` outside an `Authorization`
  header is masked only when it carries a digit or a base64 symbol; labels are
  English and Norwegian only; `{"name": "password", "value": ...}` pairs are not
  covered by the object-key rule (only the credential header names above are); neither are cookies dumped as `{"name": "sid", "value": ...}`, where the cookie's own name is the only context (field facts cover them in S1 and S3).
- **Contextual rules over-mask on purpose** under `Durable`: `password: must
  contain 8 characters` loses one word.
- **Leaves of any size are scanned whole.** The plan sketched a 64 KiB leaf cap;
  truncating would break extraction and skipping would leak, so the cap is not
  implemented. The engine is linear-time and the 1 MiB budget test (run in release by the CI step named above) is the guard.
  A JSON value nested deeper than 256 levels is replaced whole (fail closed).
- **Q10 (does Chromium mask password values in the accessibility tree) is
  answered in S1**, before it merges. Nothing here depends on the answer.
- **Screenshots and OCR text** are S5 and the Model Plane (S4); this module is text.
