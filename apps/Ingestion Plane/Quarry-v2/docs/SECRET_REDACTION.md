# Secret redaction at capture (WS12)

One deterministic, versioned rule module, `quarry_core::redaction`
(`crates/quarry-core/src/redaction/`), decides what a password, an OTP, a card
number or a token looks like and replaces it with a marker. It exists so that
one set of rules is applied before anything Quarry captures reaches a caller, an
artifact, an event, a receipt, a log or a model prompt, instead of each of those
places growing its own filter.

Rule set: `RULE_VERSION = "qr-redact/4"` (see "Version history"). Marker: `[redacted:<kind>]` with
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
| S5 | screenshots, by element rectangle, fail closed | built (U52), not run |
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
| `shape.jwt` | three dot-separated base64url segments starting `eyJ` (in any case: an element fingerprint keeps its text lower-cased) | `token` |
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
| `json.key` (header rule, **both profiles**) | the value under an object key naming a credential header (any case): `Authorization`, `Proxy-Authorization` (`credential`), `Cookie`, `Set-Cookie` (`secret`), `X-Api-Key` (`key`), `X-Auth-Token`, `X-Csrf-Token`, `X-Xsrf-Token`, `X-Csrftoken` (`token`); and the `value` of a `{"name": "Cookie", "value": ...}` or `{"key": ...}` pair as HAR and CDP write headers, or of a pair whose name is a session cookie name (`sid`, `ssid`, `connect.sid`, `JSESSIONID`, `PHPSESSID`, any `*session_id`; `secret`). The whole value is replaced, scheme included, **whatever its shape**: an object or array under one of these keys is masked as a unit (over-masking by design: the key is the only context the walk has and a credential can sit in any child; a non-header field that happens to be called `authorization` loses its content) | per header |
| `json.key` | the value under an object key that names a secret (`password`, `credential`, `api_key`, `private_key`, `token`, `accessToken`, `secret`, `session_id`, `cookie`, env-style `*_KEY`); under `Durable` the same names also mask the `value` of a `{"name": "password", "value": ...}` or `{"key": ...}` pair. Counters (`max_tokens`, `token_count`) and booleans survive; a number, array or object under such a key is replaced whole | per key |

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
line, ...), 10 JSON cases and 30 field cases. The positive, negative and JSON cases list `applies_to`
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
- `qr-redact/4` (D1-030): the three gaps the `qr-redact/3` review left. A `{"name": "password", "value": ...}`
  pair is masked under `Durable` (the name goes through the key rules, so `api_key` and `accessToken` pairs
  too), a session-cookie pair (`sid`, `JSESSIONID`, ...) under both profiles, and `X-Csrf-Token`,
  `X-Xsrf-Token` and `X-Csrftoken` join the credential headers (keys and pairs, both profiles). The `eyJ`
  prefix of `shape.jwt` is matched in any case, because the element fingerprint's `normalized_text` is the
  lower-cased element text and kept a token the other fields masked. The golden gained four cases.

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
  English and Norwegian only; a `{"name": "password", "value": ...}` pair is masked under `Durable` only
  (as the `password` key is), so `Live` leaves it; a session cookie is recognised by a short list of names
  (`qr-redact/4`), so a cookie with an application-specific name (`auth`, `login`) is not masked unless its
  name matches a key rule under `Durable`.
- **Contextual rules over-mask on purpose** under `Durable`: `password: must
  contain 8 characters` loses one word.
- **Leaves of any size are scanned whole.** The plan sketched a 64 KiB leaf cap;
  truncating would break extraction and skipping would leak, so the cap is not
  implemented. The engine is linear-time and the 1 MiB budget test (run in release by the CI step named above) is the guard.
  A JSON value nested deeper than 256 levels is replaced whole (fail closed).
- **Q10 (does Chromium mask password values in the accessibility tree) is
  answered in S1**, before it merges. Nothing here depends on the answer.
- **Screenshots and OCR text** are S5 and the Model Plane (S4); this module is text.

## WS12.S1: field-facts probe and AX and target masking (U17, built, NOT run, 2026-10-23)

Built to scope and compile-checked. Nothing is deployed, the Q10 answer is not in, and no test has been run. The browser projection now masks values before they leave the driver (`native_page_projection`).

- **Code.** `crates/quarry-browser/src/field_probe.rs` (pure masking plus a cfg-gated probe), `field_probe_live.rs` (live test, included in `chromiumoxide.rs` tests), `lib.rs` (`ProjectionRedaction`, `BrowserNativeProjection.redaction`, `BrowserNativeTarget.field_kind` with `serde(skip)`). `quarry-runtime/src/test_site.rs` serves `GET /secrets-lab`.
- **What the probe reads.** Attributes only: `type`, `autocomplete`, `name`, `id`, `aria-label`, `placeholder`. Never a value. `DOM.describeNode` first; on failure, `DOM.resolveNode` plus `Runtime.callFunctionOn` (released after the read). Controls are probed concurrently and the whole run is bounded at 400 ms. At most 64 editable controls are probed.
- **What gets masked.** Editable roles (`textbox`, `searchbox`, `combobox`, `spinbutton`) with a value. A value the classifier calls sensitive becomes its marker (`[redacted:password]`, `card`, `phone`, `otp`, `secret`). A value the probe read as not sensitive is kept, then passes the shape rules. An editable value with no facts (probe off, timeout, CDP error, over the cap) becomes `[redacted:secret]`: fail closed. Names and all other text pass the shape rules (Live) only; names are never masked by field kind.
- **Flag.** `QUARRY_BROWSER_FIELD_PROBE`, default on. `off` (also `0`, `false`, `no`) skips the probe and masks every editable value. Any other value reads as on.
- **Stamp.** `ProjectionRedaction { probe, probe_ms, values_masked, by_rule }` (counts and rule ids only). It is crate-internal and not on the wire. WS12.S2 builds the observation-level stamp from it.
- **Q10 (not answered).** `scripts/q10/ax-password-probe.mjs` and its fixture page `scripts/q10/secrets-lab.html` are the recipe. Run the script against the pinned Chromium (Node 22+) before S1 merges, then commit its output as `crates/quarry-browser/tests/fixtures/ax-password-probe.json` (not produced yet). The answer decides between describe-first and the resolve path, and whether the AX value of a password field is `plain`, `bullets` or `absent`.
- **Tests written, not run.** `cargo test -p quarry-browser projection field_probe` (pure, in `field_probe.rs`). Live, Chromium required: `CHROMIUMOXIDE_TEST=1 QUARRY_BROWSER_NO_SANDBOX=1 CHROME=/usr/bin/chromium cargo test -p quarry-browser --features chromiumoxide ax_field_probe_live -- --ignored --nocapture`.
- **Rollback.** `QUARRY_BROWSER_FIELD_PROBE=off` masks every editable value (more masking, no probe). Otherwise revert the commit. Nothing outside the browser projection reads the new fields yet.
- **Honest limits.** A text box with no label, no autocomplete and no Luhn-valid number is kept as text. A secret in static text is masked by shape only. The 400 ms budget and the probe cost are not measured. Child-frame nodes that the probe cannot reach are masked (fail closed).

## WS12.S2: observation-level masking, stamp, events, errors, mode boot (U44, built, NOT run, 2026-10-24)

Built to scope and compile-checked. No test suite has run, nothing is deployed, and the stamp has not been seen on a live stack.

- **Where it lives.** `crates/quarry-runtime/src/observation_redaction.rs` (the `Redactor`, `action_for_event`, `event_url`, `redact_json_artifact`); the calls are in `ObservationRunner::execute` (`observation.rs`). The rules and the version are unchanged: `RULE_VERSION` stays `qr-redact/3` (S2 adds no rule).
- **What the caller gets (Live).** The visible-text snippet, every interactive element's `text`, `aria_label`, `accessible_name` and `placeholder`, the click-map names, the text inside each element fingerprint (its `fingerprint_id`, a hash, is not touched, so a snapshot target still verifies), the page title, console lines, open dialogs (message and prompt default), extracted field values (shapes only, so a profile for ordinary page data still works), challenge evidence, and the `url` and `proof_bundle.source_url` (userinfo only; the query stays because the caller needs a working address). ZDR runs are masked the same way: Live applies to everything returned to any caller.
- **Native projection.** `native_page_projection` already masks its values in the driver (S1). The runner folds the driver's counts into the stamp. A projection no masking pass produced (`redaction` is `None`: a driver without the probe) fails closed here: every editable control's value is `[redacted:secret]` and every other text passes the shape rules.
- **What leaves the process (Durable).** The `ActionStarted` event carries the action with typed text masked by the field kind of the control it went to (`typed_field_kind`, read from the snapshot the action names before the action replaces it): a password, OTP, card, phone or unprobed field becomes `[redacted:<kind>]`; text for any other control keeps its words but passes the Durable rules. A raw `type` action names its control by selector only, so the words in the selector decide (`input#password` is a password). The `ObservationReady` event `url` loses secret query and fragment values and userinfo. A JSON artifact (`evidence_delta`) passes `redact_json(Durable)` before it is written. The `PageFetched` and `PageFailed` event `error` text passes Durable.
- **Errors.** `QuarryError::redacted()` masks the message and every string in `details` (Live) and keeps the code, status and retryability. `agent_routes::types::driver_err` applies it, so every driver or target error body is masked.
- **The stamp.** `BrowserObservation.redaction` is a `RedactionStamp` (rule version, `live`, the mode, `values_masked`, `by_rule`, probe state and `probe_ms`); `ProofBundle.redaction_rule_version` is the rule version string. They carry counts and static rule ids only, no content and no lengths. The AX nodes are counted once; the executable targets, which carry the same text, are not counted a second time. One `info!` per observation with `values_masked > 0` logs the counts (`quarry.redaction.observation`).
- **Modes.** `QUARRY_EDGE__REDACTION_MODE` (compose: `QUARRY_REDACTION_MODE`, default `enforce`). `shadow` counts and returns the input unchanged; `off` does nothing; both refuse to boot when `ENVIRONMENT` is `prod` or `production`; an unparsable value refuses to boot anywhere (`redaction_boot.rs`). The mode is pinned once at boot.
- **Tests written, not run.** `cargo test -p quarry-runtime observation_redaction` (the pure functions and the runner through a fixture driver), `cargo test -p quarry-edge --lib redaction_boot`, `cargo test -p quarry-edge --test redaction_wire`.
- **Honest limits.**
  - A raw `type` action into a control whose selector says nothing is masked by shapes only; a plain word typed there is kept in the event.
  - A scanner target (a driver with no native projection) has no `type` attribute: an input or textarea its labels do not explain is treated as `Secret`, so typed text for it is always masked in the event.
  - Element `selector` strings are built from attribute values and are not masked (a selector is how an action finds the control).
  - `network_summary` URLs are not masked at the observation level.
  - The `shadow` stamp counts what enforce would mask. It does not say which values were false positives; that is a person reading a page.

## WS12.S5: screenshot masking by element rectangle, fail closed (U52, built, NOT run, 2026-10-24)

- **What it does.** Every screenshot the Chromium driver takes is scanned first for sensitive form fields, and the rectangles of the ones that are on screen are painted over (`#1f2937`, 4 CSS px of padding) before the bytes leave the driver. The masking sits at the capture call, so the artifact store, the vision sidecar, the proof bundle, the explicit `screenshot` action and the scrape capture all get the masked image without touching their write sites. `quarry-browser/src/sensitive_rects.rs` is the read-only scan (one `Runtime.evaluate` over `input`, `textarea`, `select` and `[contenteditable]`, through open shadow roots and same-origin iframes, adding frame offsets); the Rust classifier `quarry_core::redaction::field::classify` is the authority on which fields are sensitive, the script decides nothing. `quarry-browser/src/screenshot_mask.rs` turns the rectangles into device pixels (scroll, clip origin, device pixel ratio, padding, clamped to the image) and paints them with the `png` crate. `BrowserDriver::screenshot_masked` returns `MaskedScreenshot { bytes: Option<Bytes>, state, rects, oopif_frames, mask_ms, note }`; the default implementation calls `screenshot` and reports `none`.
- **The rules.** A sensitive field that is rendered with a usable rectangle is masked. A sensitive field that is rendered but has no usable rectangle (zero-size box, a probe error, a scan that failed or timed out, a walk cut off by the node cap) withholds the image. A field that is not rendered (`display:none`, `visibility:hidden`, `type=hidden`) is ignored. The scan runs before the capture and the scroll offset is read again after it; a scroll in between re-scans once, a second scroll withholds. An image over 40 megapixels, a PNG that is not 8-bit RGB or RGBA, and a JPEG that has something to mask are withheld. An image with nothing to paint is returned untouched.
- **What withheld means.** `screenshot_masked` returns `bytes: None` and the note `screenshot_withheld:<cause>` (`sensitive_field_unresolved`, `image_too_large`, `image_unsupported`, `mask_failed`). In an observation: no `screenshot_artifact_id`, no screenshot artifact row, no vision sidecar input (the sidecar and image artifact writes are skipped), the previous-frame memory is cleared, and the stamp says `screenshot: "withheld"`. The explicit `screenshot` action, `screenshot()` itself and a scrape capture return `SECURITY_BLOCKED` (403) with `details.reason` set to the same note. A PDF cannot be painted over: `pdf()` and `capture_pdf()` return `SECURITY_BLOCKED` with `details.reason` `pdf_withheld:sensitive_field_value` when a rendered sensitive field holds a value, and `pdf_withheld:sensitive_field_unresolved` when the scan failed or a sensitive field cannot be placed.
- **The stamp.** `redaction.screenshot` is `none`, `masked` or `withheld`; `redaction.rects` counts the painted rectangles, `redaction.oopif_frames` the cross-origin frames the scan could not look into, `redaction.mask_ms` the time spent scanning and painting (outside the capture). Counts only: no rectangle and no offset leave the process.
- **Flags.** `QUARRY_BROWSER_SCREENSHOT_MASK=on|off` (default `on`; `off` is honoured only when `ENVIRONMENT`/`APP_ENV` is not `prod` or `production`). `QUARRY_BROWSER_SCREENSHOT_OOPIF=allow|mask` (default `allow`: cross-origin iframes are counted and left as they are; `mask` blanks every cross-origin iframe rectangle, and a frame with no usable rectangle withholds). No migration; rollback is the previous image.
- **Tests written, not run.** `cargo test -p quarry-browser screenshot_mask` (`mask_png_blocks_exact_rect_and_leaves_rest_untouched`, `mask_png_rejects_oversize`, `rect_scaling_uses_device_pixel_ratio`, `padding_clamps_to_image`, the plan and origin tests), `cargo test -p quarry-browser sensitive_rects`, `cargo test -p quarry-runtime observation withheld_screenshot`, and the live lane `CHROMIUMOXIDE_TEST=1 cargo test -p quarry-browser --features chromiumoxide screenshot_mask_live -- --test-threads=1 --ignored --nocapture` (basic, device pixel ratio 2, scrolled, same-origin iframe, withheld, hidden password and PDF).
- **Honest limits.**
  - **Cross-origin iframes (out-of-process frames) cannot be scanned.** The default is `allow`: a sensitive field inside one is not masked, and the stamp only counts the frames. `QUARRY_BROWSER_SCREENSHOT_OOPIF=mask` covers it by blanking the whole frame. This is a recorded limit, not a fix.
  - **The live view (`live_frame`) is not masked.** It is the person's own screen, not an artifact.
  - **Only the Chromium driver scans.** The Browserless, Browserbase and Kernel drivers report `none` and return their images as they were.
  - The scan classifies from attributes and labels (the same rules as the accessibility masking). A card number typed into a field with no card attribute or label is found by its value (Luhn) only for the first 40 characters; a secret typed into an unlabelled text field is not recognised.
  - The scan runs in the page's own JavaScript world: a hostile page can lie to it. The mask protects against typed secrets reaching a stored image, not against a page built to defeat it.
  - Elements under a CSS transform (a scaled iframe, a rotated field) are placed by their unscaled box; padding covers small differences only.
  - One extra round trip per screenshot (two when a sensitive field is on screen). `mask_ms` is measured, not asserted; the 150 ms p95 target for a 1280 by 800 capture has not been measured.
