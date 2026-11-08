# W5 verification 3 (S4 clear-data and ADR-5 fence, quarryv2-next)

Date 2026-11-08. Worktree `.claude/worktrees/vigorous-perlman-e7ae5b`, branch `quarryv2-next`. No writer file edited. Local only.

Tip note: the brief said tip `f55beef14`. That commit is the base of S4. The real tip is `bb4c58968`, with four S4 commits on top: `eec751381` (quarry-browser), `de1c938e6` (quarry-edge), `30dbc323f` (gateway), `bf24d35a0` (verevonv3), plus `bb4c58968` (progress note). I verified the tip, and diffed `f55beef14..bb4c58968` for the fence.

Dirty tree, not mine: `quarry-runtime/src/step_receipts.rs` is modified, and the diff is line endings only (one real line differs by CR). Untracked `sdks/typescript/src/**` and other `.plan-work` files. None were staged or touched.

## Ran vs read

| Item | How |
|---|---|
| Tests below | RAN, native Windows, `CARGO_TARGET_DIR=C:\qv2t`, `-j 2`, in the worktree itself |
| Fence greps | RAN (`git grep` at HEAD) |
| Base comparison for the one failure | RAN, temporary detached worktree at `f55beef14` (removed after) |
| Live stack, browser, gateway, SPA | NOT RUN (out of scope) |

## Test counts

| Target | Pass | Fail |
|---|---|---|
| quarry-browser `--lib` | 214 | 0 |
| quarry-browser `egress_deny_list` | 8 | 0 |
| quarry-browser `open_web_policy` | 7 | 0 |
| quarry-browser `profile_cdp_spike`, `ssrf`, doc-tests | 0 | 0 |
| **quarry-browser total** | **229** | **0** |
| quarry-edge `--lib profile` | 25 | 0 |
| quarry-edge `--lib clear` | 4 | 0 |
| quarry-edge `profile_clear_data` | 7 | 0 |
| quarry-edge `profile_owner` | 8 | 0 |
| quarry-edge `profile_roundtrip` | 11 | 0 |
| quarry-edge `profile_attach` | 10 | 0 |
| quarry-edge `api_key_fence` | 7 | 0 |
| quarry-edge `profile_guard` | 7 | **1** |
| **quarry-edge targeted total** | **79** | **1** |

(The `--lib clear` filter overlaps `--lib profile` by some tests; the 79 counts both runs as run.)

### The one failure: `profile_guard::import_over_cap_is_413`

`ConnectionAborted` (os error 10053) on the 5 MiB POST. The server sends 413 and closes before it reads the body, and Windows resets the client's send. It failed 5 of 5 on the tip. It fails the same way, same error code, on a clean checkout of `f55beef14`, so S4 did not cause it. It matches the S4 progress note. The S4 diff does not touch `routes.rs` body limits or the import handler. The test passed on the Linux `qtest` path in earlier notes (D-D-B2: profile_guard 8 passed). Verdict: pre-existing, Windows only, not a W5 regression. It needs a Linux or Docker run to see it green.

## S4 behaviour, as covered by tests (read plus run)

- `profile_clear_data` 7/7: default off gives 501 `profile_clear_data_disabled`, dry run is the default, owner-only (also on an org-shared profile), live-run holder refused, snapshot keeps UA, viewport, locale, timezone.
- `profile_guard` unit tests (in `--lib profile`): `clear_data_is_owner_only_even_on_an_org_shared_profile`, `zdr_wind_down_is_list_delete_and_clear` (clear is allowed for a ZDR org, as erasing).
- `quarry-browser` `session.rs` tests: counts carry no names or values; `without_browsing_data` clears cookies, web storage and IndexedDB (flat and per origin) and keeps the device settings.

Known residual from the S4 note, unchanged: a run already removed from the registry but not yet released can re-save its jar after a clear.

## ADR-5 fence (HOLDS)

| Check | Result |
|---|---|
| `stealth` (Quarry crates, services, gateway src, SPA src) | **32 hits in 11 files**, identical to W5-VERIFY2 (2 + 3 + 1 + 2 + 2 + 1 + 2 + 1 + 1 + 15 + 2). All are reserved-key refusals, a fail-closed typo test string, a comment, the Go deprecated alias `stealth -> careful` and its tests, and pass-through of the Go preset name. No mechanism. Zero hits in S4 files |
| captcha (crates + services) | 135 hits in 9 files, all detection or classification (`challenge.rs` 92, `pipeline.rs` 11, `observation.rs` 10, and contracts/error/tests/search/fallback). No solver vendor, no `solve` + captcha, no bypass of a challenge. The `bypass` hits are cache and proxy-loopback only |
| consent (crates + services) | 25 hits in 5 files: `challenge.rs` 21 (`CONSENT_PHRASES`, `CMP_MARKERS`, classification to `consent_required`), `actionability.rs` 1 (a test string), `contracts.rs` 1, `tests/contracts.rs` 1, `answer.rs` 1. No click or accept action in `crates/` or `services/`. Unchanged from VERIFY2 |
| S4 diff (`f55beef14..bb4c58968`) added lines hitting stealth, captcha, solve, consent, navigator, webdriver, fingerprint, impersonat | **0**. The only `user_agent` hits are the clear-data doc text and tests that assert the UA is **kept** |
| UA identities | **Unchanged: 5 identities across 6 production sites**, plus 3 fixed browser UAs in `quarry-tls`. Sites: `config.rs:337` and `answer.rs:733` (`Quarry/2.0 (+https://triodelab.com/quarry)`), `policy.rs:101` (`Quarry/2.0`), `main.rs:1636` (`Mozilla/5.0 (compatible; QuarryBot/1.0; +https://verevon.io/bot)`), `fetch.rs:22` (`QuarryBot`), `robots_gate.rs:52` (tokens). `config.rs` moved from line 333 to 337 only because S4 added 4 lines above it |
| `FingerprintRotator`, `navigator.webdriver`, `setUserAgent`, `puppeteer-extra` | None. The `userAgent` pattern hits are `quarry-tls` (11), `tls_driver.rs` (4), session capture/restore of the stored UA, and robots test fixtures |

Open items, unchanged and not S4: D-W5-1 (owner UA string) still open, so S3 stays blocked. D-W5-2 (what the TLS tier presents). FENCE-1 on `main` (desktop-browser auto cookie-banner dismissal, reject-first, no receipt) and FENCE-2 (stale committed worktree snapshot) are not on this branch's diff and not changed here.

## Verdict

S4 clear-data: tests green, 0 failures attributable to S4. ADR-5 fence: holds, with no new hits. One Windows-only pre-existing failure (`import_over_cap_is_413`).
