# W5 verify 2 (fresh verifier): W5-S4 clear-data and the ADR-5 fence

Date 2026-11-08. Worktree `.claude/worktrees/vigorous-perlman-e7ae5b`, branch `quarryv2-next`, HEAD `f55beef14` (3 W5 commits over `cc8c43389`: `f90ed49a3` 074 provider half, `79843694c` 071a, `f55beef14` S1/S2/S6 notes). No writer file edited. Local only.

## 1. W5-S4 clear-data: NOT BUILT (nothing to verify)

| Check | Result |
|---|---|
| `git grep -i "clear.data\|clear_data\|clearData"` in `apps/Ingestion Plane` and `apps/Frontend Plane` | 0 code hits (one unrelated prose hit in a lab fixture, `web-rerank/results.v1.jsonl`) |
| `profile_routes.rs` | only "An explicit empty string clears the name" (rename path, line 459). No clear route, no `SessionSnapshot` wipe |
| Commit history on this branch (`git log --grep clear`) | no S4 commit |
| Working tree (`git status`) | no S4 edit. Only the `step_receipts.rs` CRLF churn, `rb.patch`, `next-slices.md` and the untracked SDK files, none mine and none S4 |
| `src/shared/actions` action contract | none for clear-data |
| Other worktrees checked | `gracious-gagarin-087139` (branch `quarryv2`, W4 work, dirty tree): no clear-data either. No branch name contains `w5` |
| `.plan-work/quarry-w1w2/W5-S4-progress.md` | does not exist (S1, S2, S6 do) |

Verdict: W5-S4 is 0 % built and 0 % proven in every place I could find. If a writer is mid-flight it has written nothing to disk yet. Test counts for S4: none (no test exists). The ORCH row stays OPEN.

## 2. ADR-5 fence on the current tree (tracked files, `git grep -i`, crates + services + SPA + gateway, markdown/json/yaml excluded)

| Pattern | Result |
|---|---|
| `stealth` | EMPTY of mechanism. Hits: reserved-key refusals (`quarry-core/scrape.rs` 2, `crawl_request.rs` 2, `routes.rs` 1, `scrape_contract.rs` 2), a fail-closed typo string (`escalation.rs` 1), a comment (`quarry-tls/lib.rs` 1), Go deprecated alias `stealth -> careful` (`presets.go` 2, `presets_test.go` 15), and gateway/SPA pass-through of the Go preset name (`knowledge/products.rs` 2, `knowledge/quarry.rs` 3, `knowledge-client.ts` 1) |
| `captcha` | EMPTY of solving. All hits classify or surface a challenge: `challenge.rs` (86), `fallback_driver.rs` 4 (refuses escalation on a strong challenge), `pipeline.rs` 7, `observation.rs` 3, `contracts.rs`, `error.rs`, error-copy and tests in the SPA. The SPA `Turnstile*`, `auth/public.rs` and `auth-client.ts` hits are Verevon's own sign-in widget, not scraping. No solver vendor (2captcha, capsolver, anticaptcha): 0 |
| `consent` | EMPTY of auto-accept. Quarry: `challenge.rs` 21 (`CONSENT_PHRASES`, `CMP_MARKERS`, classification only), `actionability.rs` 1, `contracts.rs`/`answer.rs` 1 each. The rest (about 70 files) is Verevon product consent (OAuth scopes, connectors, analytics), unrelated to browsing |
| `navigator.webdriver`, `FingerprintRotator`, `CONSENT_DISMISS`, `stealth_level` | 1 hit, prose only: `fingerprint_rotation.rs:6` "the round-robin `FingerprintRotator` this module used" (removed) |
| `stealth.rs` | not in `git ls-files` |
| ADR-5 BUILD_DEFAULT | `escalation.rs:34` = `Honest`; unknown env value fails closed to `honest`; only `TransportError` may change identity |

The fence holds as a literal grep: nothing in a code path spoofs, solves or accepts.

### Observation for the owner (not a fence breach, not new in W5)

`quarry-tls` still carries three browser UA strings (`TlsProfile::{Chrome,Firefox,Safari}::user_agent()`, `lib.rs:65-72`) and the TLS driver sends them by default (`lib.rs:187`, `tls_driver.rs:106`). Under `honest` the plan does not pick the TLS tier from a block (`plan_from_signals_with` ignores `prior_block_signals`; `fallback_driver` and `fetch` gate on `escalation_allowed`), so this is reachable only by an explicit `tls_profile`, a transport failure, or the chain's non-block routes. It is still a browser-identity UA, which guard rule 1 says the UA may only move away from. D-W5-1 and D-W5-2 should say what the TLS tier presents. The earlier W5-VERIFY note said "no UA pool or rotation code"; there is no rotation, but the three strings are a fixed set.

## 3. UA and token count (D-W5-1)

Distinct honest-identity strings and tokens on `quarryv2-next` (unchanged from W5-ORCH):

| # | String | Where |
|---|---|---|
| 1 | `Quarry/2.0 (+https://triodelab.com/quarry)` | `quarry-edge/src/config.rs:333` and `quarry-runtime/src/answer.rs:733` (same string, 2 sites) |
| 2 | `Quarry/2.0` | `quarry-runtime/src/policy.rs:101` (hashed, never applied) |
| 3 | `Mozilla/5.0 (compatible; QuarryBot/1.0; +https://verevon.io/bot)` | `quarry-edge/src/main.rs:1636` (HTTP/3 driver, behind `http3` feature) |
| 4 | robots token `QuarryBot` | `quarry-runtime/src/fetch.rs:22` `DISCOVERY_USER_AGENT` |
| 5 | `ROBOTS_PRODUCT_TOKENS = ["quarrybot","quarry"]` | `quarry-runtime/src/robots_gate.rs:52` (test literal `quarrybot` also in `crawl_sitemap.rs:980`) |

**5 unaligned identities across 6 production sites, 2 domains (triodelab.com, verevon.io), 3 product tokens (`Quarry/2.0`, `QuarryBot/1.0`, `QuarryBot`).** Plus the 3 browser UAs in `quarry-tls` (section 2). No `quarry-core` constant exists yet.

**D-W5-1: STILL OPEN (owner string not given).** No commit, note or config on this branch or in `W5-*` notes records an answer. W5-S3 (067 slice 1) therefore stays BLOCKED. Recommended and unchanged: `QuarryBot/2.0 (+https://verevon.io/bot)`, robots tokens `quarrybot` and `quarry`.

## 4. Tests

Targeted only: `CARGO_TARGET_DIR=C:\qv2t`, `-j 2`, `cargo test -p quarry-runtime --lib -- escalation driver_plan robots_gate`. See the result line appended below.

(result appended at the end of this file)

## Test result

`cargo test -j 2 -p quarry-runtime --lib -- escalation driver_plan robots_gate` (CARGO_TARGET_DIR=C:\qv2t): **47 passed, 0 failed, 0 ignored** (1078 filtered out). Includes `build_default_matches_the_documented_default` and `the_env_var_reaches_current_and_legacy_is_the_rollback`. The first run waited about 15 min on the shared C:\qv2t build lock held by other sessions' cargo runs, then compiled. No S4 test exists, so none was run. Log: `%TEMP%\opencode\w5v2-runtime.log`.

