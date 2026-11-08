# W4 verify 3: E4.4-a green at tip b1753710a (2026-11-08)

Worktree `gracious-gagarin-087139`, branch quarryv2, tip `b1753710a` (`a4ecf5dca` goldens, `b1753710a` 400 unsupported_option). Writer files untouched. Targeted tests only, `-j 2`, `CARGO_TARGET_DIR=C:\qv2t-w4v2`, `CARGO_INCREMENTAL=0`.

## Verdict

E4.4-a is green. The only failures are 4 + 5 that exist for reasons the slice does not touch (below). Geolocation is not an ADR-5 breach, but it is a scope change that needs the owner's yes (below).

## Cargo (Quarry-v2)

| Command | Result |
|---|---|
| `quarry-core --lib -- page_emulation scrape` | 67 passed |
| `quarry-core --features jsonschema --test scrape_schema` (goldens, no UPDATE_SCHEMAS) | 2 passed |
| `quarry-edge --lib -- scrape_contract scrape_actions page_emulation` | 42 passed (includes the 3 new tests from `b1753710a`: route-level 400 for no browser, per-field helper, stream refuses only while switch on) |
| `quarry-edge --test scrape_actions` | 9 passed |
| `quarry-edge --test scrape_max_age` / `scrape_max_age_off` | 15 / 3 passed |
| `quarry-edge --test scrape_capture` | 4 passed |
| `quarry-edge --test change_webhook_proptest` | 4 passed |
| `quarry-edge --test change_diff_route` | 0 tests (file is `#![cfg(feature = "postgres-queue")]`, not run) |
| `quarry-edge --test scrape_records_durable` | **0 passed, 4 failed** (see A) |
| `quarry-runtime --lib -- page_emulation_tests` | 5 passed |
| `quarry-runtime --test ai_scrape` | 41 passed |
| `quarry-browser --lib -- page_emulation profile_restore capability_wire` | 25 passed |
| `quarry-browser --features chromiumoxide --lib -- page_emulation profile_restore` | 25 passed, 3 ignored (live Chromium) |

Cargo total: 242 passed, 4 failed (pre-existing, A), 3 ignored.

## Node (openapi parity)

| Test | Result |
|---|---|
| `openapi-scrape-parity` | 10/10 |
| `openapi-challenge-parity` | 8/8 |
| `openapi-no-bare-null` | 2/2 |
| `openapi-route-coverage` | 8/10 (2 fail, see B) |
| `openapi-internal-guard` | 7/10 (3 fail, see B) |

Node total: 35 passed, 5 failed (pre-existing, B). Scrape parity (the one the slice can affect) is green: the three new options and `page_emulation` are in `openapi.yaml`.

There is no CHANGELOG file and no changelog test in Quarry-v2 (`find` on `*change*` found only the change-diff and challenge sources). I treated "changelog" as the change-tracking tests: `change_webhook_proptest` ran, `change_diff_route` is postgres-gated and ran 0.

## Failures, none caused by the slice

A. `scrape_records_durable` (4 tests): `POST /v1/scrape` answers 500 `INTERNAL`, `fs write ...\page=blake3:<hash>\html.html: Mappenavnet er ugyldig. (os error 267)`. The filesystem artifact store builds a directory named `page=blake3:...`. A colon is not valid in a Windows directory name. This is a Windows-only defect in the store key. The test file was last touched by WS13.4a, the slice diff does not touch it or the store, and the 500 comes from the store write, before any emulation code. I did not run it on the pre-slice commit (no disk for a second build), so "pre-existing" rests on that reading. It would pass on Linux CI. Suggested fix, separate slice: encode `:` in the store path segment.

B. Internal-spec guards (5 tests): the Go control service registers 10 `/v1/internal` routes against 8 documented, and the internal route count pin and SDK-internal checks follow from the same drift. The slice touched no route registration (`git diff` of `routes.rs` adds no `.route(`/`get(`/`post(`), no `openapi-internal.yaml`, no `services/quarry-control`. The drift dates from W3 (`b43bd4886`, `154529c2a`).

## Geolocation ruling: is `location` an ADR-5 breach?

**No breach of ADR-5. It needs one owner confirmation because it is outside the ranked spec.**

What ADR-5 decided (option (a), honest identity; recorded in W5-ORCH section 1, "do not reopen"; `escalation.rs` pins `Honest`). Its five guard rules:
1. no UA spoofing or override toward a browser identity;
2. no fingerprint, `navigator.webdriver`, canvas or WebGL masking, no stealth plugins;
3. no identity change (proxy rotation, driver fallback, TLS swap) triggered by a block status;
4. no CAPTCHA solving, no consent auto-accept;
5. no `prior_block_signals` forcing a tier in honest mode.

Check against the slice diff (`de9678470~1..HEAD`, crates only): no hits for `user_agent`, `setUserAgent`, `Accept-Language`, `webdriver`, `canvas`, `webgl`, `stealth`, `mobile`, `country`, `prior_block`, `rotat`. The code adds `Emulation.setLocaleOverride`, `setTimezoneOverride` and `setGeolocationOverride` (plus a per-origin geolocation grant). `page_emulation.rs` states that UA and `Accept-Language` are left out on purpose as ADR-5 identity changes. The three overrides are declared by the caller per request, are validated, are not set in reaction to a block, and do not change what Quarry presents on the wire (UA, headers, TLS, IP). GAP.md already lists locale and timezone emulation as allowed, "by decision (ADR-5)" being about stealth and fingerprint spoofing only. So rules 1-5 are not hit.

Why it still needs a yes: `W4-BLOCKERS-RANKED.md` (rank 1, row 45 and lines 75/113) put "`location.country` / geolocation" in E4.4-a2, held for ADR 5, and said a1 is locale and timezone only. `W4-ORCH2.md` N3 flagged the drift for an owner call "before lane b's green commit". The slice ships lat/long/accuracy anyway. My read is that the held item was `location.country` (an egress country, which is E4.4-c and ties to ADR-10 and identity), and a lat/long page override is a different thing. A site that reads `navigator.geolocation` gets a caller-chosen position. That is closer to a feature than to evasion. But it is the owner's call, not mine.

Recommended: record N3 as "keep `location` in a1, ADR-5 not engaged, `location.country` and the egress tier stay in a2/c". Residual risk: the geolocation grant is untested end to end (live Chromium tests are `#[ignore]` and did not run).

## Environment

- C: hit 0 bytes free mid-session (shared with other target dirs: `qv2t` 80 GB, `qv2t-w4v2` 30 GB, others). My first target dir `C:\qv2t-w4v3` (22 GB, mine) was deleted to continue. I reused `C:\qv2t-w4v2` and removed only its `debug\incremental` (rebuildable). Nothing else was deleted. About 10 GB free now.
- A temporary copy of the `scrape_records_durable` test (printing the 500 body) was created and removed; `git status` for Quarry-v2 is clean.

## Not done

No full workspace test, no Go test, no live Chromium test, no baseline run of A on the pre-slice commit.
