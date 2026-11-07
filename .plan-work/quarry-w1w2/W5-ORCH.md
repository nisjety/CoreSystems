# Quarry Wave 5 orchestration (E5.1 to E5.3) and Model Plane status

Date: 2026-11-07. Branch `quarryv2-next` (worktree `vigorous-perlman-e7ae5b`, tip `cc8c43389`, 2026-11-01).
Sources: build plan `docs/plans/quarry-browser-build-plan-2026-10-14.md` (W5 table at 3335-3346, sequencing 3348, E5.1 3423, E5.2 3433, E5.3 3443, ADR-5 / OQ-5 at 430-436); honest-identity status `apps/Ingestion Plane/Quarry-v2/docs/RUNBOOKS.md` "Block escalation" (535) and `docs/PROGRESS.md` (WS3.I, 2026-10-21); `.plan-work/quarry-w1w2/MP-W2W3-STATUS.md` (branch `mp-w2w3`, `8188a6ea1`); `.plan-work/quarry-w1w2/MP-ASKS.md`.

Rules: notes only, local commits only. Nothing below was run on a live stack. "Built" means the code is in the tree. "Proven" means a test or live check shows it works. A row is done only when it is proven.

## 0. Branch base (read first)

`quarryv2-next` is fully contained in `main`. `main` is 1287 commits ahead of it, and `quarryv2-next` has 0 commits that `main` lacks. `mp-w2w3` is also already in `main`. The worktree also holds uncommitted work that is not mine: `quarry-runtime/src/step_receipts.rs` is modified, the regenerated TypeScript SDK files are untracked, and so is `.plan-work/next-slices.md`. I did not touch any of it.

**Recommendation:** cut every W5 slice branch from `main`, not from `cc8c43389`. Line numbers move between the two. For example the UA sites on `main` are `answer.rs:892`, `policy.rs:112`, `config.rs:341` and `main.rs:1744`, against 733, 101, 333 and 1636 here. Fast-forwarding `quarryv2-next` to `main` is the owner's call, and it needs the uncommitted files above dealt with first.

## 1. ADR-5 enforcement

**Decided (do not reopen):** option (a), honest identity, is approved.
- In code: `quarry-runtime/src/escalation.rs:34` sets `BUILD_DEFAULT = EscalationMode::Honest`, and the test `build_default_matches_the_documented_default` pins it.
- `stealth.rs` is deleted (absent on both `quarryv2-next` and `main`).
- The `stealth` preset is now `careful`, with `stealth` kept as a deprecated alias.
- Profile restore never overrides the UA. It only records `ua_changed` (`profile_restore.rs:16`).
- Rollback is `QUARRY_BLOCK_ESCALATION=legacy` plus a restart.
- Caveat: no before/after measurement was ever taken (RUNBOOKS 535), so the flip is proven by tests only, not by live data.

**Guard rules for every W5 slice.** Breaking any one of these needs a new ADR, and the slice must not merge:
1. No UA spoofing or override toward a browser identity. The UA is only allowed to move toward the single honest constant (F-t2-067).
2. No fingerprint, `navigator.webdriver`, canvas or WebGL masking. No stealth plugins.
3. No identity change (proxy rotation, driver fallback, TLS swap) triggered by a block status. Rotation happens on transport failure only (`escalation_allowed`).
4. No CAPTCHA solving and no consent or cookie-banner auto-accept. A consent preset (F-t2-066) stays parked.
5. Do not use `prior_block_signals` to force a tier in `honest` mode.

**Sweep on 2026-11-07 (`quarryv2-next`, `apps/**/*.{rs,go,ts,tsx,js}`):** clean.
- No `FingerprintRotator`, `navigator.webdriver`, CAPTCHA solver, `CONSENT_DISMISS`, `stealth_level` or stealth-plugin hits.
- The model-gateway prompt at `http_routes.rs:2570` forbids anti-bot bypass and CAPTCHA solving. That is correct.
- One naming issue, low priority: `quarry-runtime/src/fingerprint_rotation.rs` now holds only `is_block_status` (classification, no rotation). Rename it to `block_status.rs` in a later hygiene slice. It is not an ADR breach.

**ADR-5 sub-decisions still open (owner):**
- **D-W5-1 (blocks W5-S3): the one honest product identity (F-t2-067 slice 1).** Five unaligned strings exist today:
  - `Quarry/2.0 (+https://triodelab.com/quarry)` in `config.rs` and `answer.rs`
  - `Quarry/2.0` in `policy.rs` (hashed, never applied)
  - `Mozilla/5.0 (compatible; QuarryBot/1.0; +https://verevon.io/bot)` for the HTTP/3 driver in `main.rs`
  - the robots token `QuarryBot` (`fetch.rs:22` `DISCOVERY_USER_AGENT`)
  - `ROBOTS_PRODUCT_TOKENS = ["quarrybot","quarry"]`

  Pick one product token, one domain (triodelab.com or verevon.io) and the URL of the bot-information page. Recommended: `QuarryBot/2.0 (+https://verevon.io/bot)`, with the robots tokens `quarrybot` and `quarry`. This also closes D2-218 and the token question in DECISIONS-ROUND-2026-10-27 (106, 663).
- **D-W5-2 (not blocking W5):** the per-tier identity stance. Does the Chromium tier keep its stock UA, or append the product token? This must be settled before E4.4 tiers or any request signing (plan 3436). Recommended: append the token, never present a non-Chromium identity.
- **D-W5-3 (keep parked):** F-t2-066, the consent and ad-filter preset. It would grant tracking consent on the user's behalf. Recommended: stay parked. If it is ever reopened, it needs an ADR-5 amendment, reject-first only, behind an org policy key, with every click in the receipts.

## 2. Slice assignment

The order follows plan sequencing (1): "open now, no decision".

| Slice | Items | Lane / owner | Effort | Depends | Gate | Status |
|---|---|---|---|---|---|---|
| W5-S1 | 071a: delete `WebPageViewer` (`DashboardSearchResults.tsx:147`, no importer, `allow-same-origin`). 069 step 1: allow `pdf` and `screenshot{full_page}` in the gateway `sanitize_action` (`apps/gateway/src/domains/browser.rs:4486-4550`) | Frontend (gateway + SPA) | < 1 d | none | none | OPEN, assign first |
| W5-S2 | 074: `ProviderError` with fixed messages in `browserbase.rs` (raw `body` still in details at 119-124), `kernel.rs` and `browserless.rs`; a bounded JWKS stale-if-error window (must not extend a revoked key); RUNBOOKS dependency matrix | Ingestion (quarry-browser, quarry-edge auth) | ~3 d | none | none | OPEN |
| W5-S3 | 067 slice 1: one `quarry-core` UA constant replacing the five strings, plus the bot-info page | Ingestion (core, runtime, edge, compose) | ~2 d | WS14 robots gate (built, observe) | **D-W5-1** | BLOCKED on owner string |
| W5-S4 | 068 clear-data on the stored `SessionSnapshot` through `ProfileStore` (`profile_routes.rs`), with a count-first dialog. Action contract in `src/shared/actions` first. Must not touch a live run's jar. No-op under ZDR | Ingestion + Frontend | ~5 d | WS17 owner guard (built) | none | OPEN |
| W5-S5 | 070 tab-strip context menu (`BrowserChrome.tsx`) through gateway-to-Quarry tab routes | Frontend | M (~6 d) | WS9 merged | none | WAIT: confirm WS9 on `main` |
| W5-S6 | Docs: 075 deletion note (deprecation headers already in D5; signing has no premise); 077 decision note (owner ruling 2026-09-28; lists what reopens it); stale-doc rows `FIXES_APPLIED.md` and `SCRAPING_ENGINE_GAPS.md` (adblock, archive) and `GAP.md` (`blockAds`) | Docs (any lane) | < 1 d | none | none | OPEN |

Parked items (record them, schedule nothing):
- 066 (D-W5-3)
- 067 request signing (D-W5-2)
- 068 visit log (D8-5)
- 069 find, zoom and viewport (WS3 S2 per-lease design)
- 071 MCP App frame (no model-gateway resource lane)
- 072 (ADR-8)
- 073 (F-t2-036)
- 076 (WS1, WS11, plus a read-only broker grant class: Model Plane)
- 077 (owner ruling)

Model Plane has no scheduled W5 work. Its only W5 touchpoint is 076, which is parked.

## 3. Built vs proven

| Item | Built on `quarryv2-next` | Built on `main` | Proven | Evidence |
|---|---|---|---|---|
| 071a delete WebPageViewer | no | no | no | grep: still defined at `DashboardSearchResults.tsx:147` on both |
| 069 step 1 | partial | partial | no | SPA `BrowserArtifactMediaKind` already includes `'pdf'` (`browser-session.ts:603`); the gateway still rejects `pdf` ("Unsupported browser action", `browser.rs:4549`); `screenshot` passes through unvalidated |
| 068 clear-data | no | no | no | no clear route in `profile_routes.rs` |
| 070 chrome polish | no | not checked | no | waits on WS9 |
| 074 ProviderError + JWKS | no | no | no | no `ProviderError` and no stale-if-error in `quarry-browser`/`quarry-edge` on either branch |
| 067 slice 1 UA constant | no | no | no | five strings, listed above, on both branches |
| 075 / 077 notes | no | no | n/a | no note found |
| ADR-5 (a) mechanism (W1, prerequisite) | yes | yes | tests only | `escalation.rs` pin test; 21 runtime tests (PROGRESS 2026-10-21); no live measurement |

**Wave 5: about 5 % built (effort-weighted: about 1 of about 20 scheduled days, the SPA `pdf` media kind), 0 % proven. 0 of 6 slices done.**

## 4. Model Plane status (synced from `MP-W2W3-STATUS.md`, `mp-w2w3` @ `8188a6ea1`, 2026-11-02)

- **Merge state:** `mp-w2w3` is an ancestor of `main`, so it is merged. It is not in `quarryv2-next`, which is older.
- **W2 rows: 30 built, tested in process.** These cover the WS11 S0-S7 execution-core and model-gateway halves, WS16.S1/S8, WS10.S2/S4/S5/S6/S9 (C6), WS7.L, WS8.G/J, WS9.E, WS12.S4a/b, INNH-10 MP-1/2/3, F12 and the U61 broker migration. The only partial row is WS10.S9 (C7 is Frontend's; soak not run). U61 was also checked live, read-only, on 2026-11-01.
- **W3 rows: 13 built, compile-checked only.** These are E3.2-a/c/d/e1/e2, E3.3-f/i/l, E3.6-8, E3.7.10 and E3.9.3/4. The user tests them after merge.
- **Final regression at the branch tip, 2026-11-02:**
  - execution-core lib: 1763 passed, 2 failed (1 load flake; 1 real bug fixed in `4eae88bd3`)
  - model-gateway lib: 2650 passed, 1 failed (pre-existing, also at base)
  - session-core: 705 passed, 0 failed
  - mp-* crates and Go services: all pass
  - zdr-contract-sync: 140 pass, plus 10 pre-existing Windows CRLF reds
- **Proven live: nothing beyond the U61 read check.** Every new behaviour is behind a default-off flag. Live Quarry, live broker, real JetStream consumers, real Auth Core key tokens and the 90 % routing-compliance probe are all unverified.
- **Open at hand-over (from MP-ASKS):**
  - (a) agent-action-admission golden re-vendor (W2-FE-L5a)
  - (b) broker `RenewGrant` for Quarry's service principal on `human_browse` (W2-U39b): yes or no decision
  - (c) `web_policy` quarry-token-claims golden re-vendor (W2-CP-WS7I)
  - (d) `api_key_revoked` transport, control-plane audit stream or shared bus (INNH-10)
  - (e) the Quarry tab-mark route is not built, so WS8.J mark is a logged 404

  On `main` the canonical pins for (a) and (c) now hold neither the old nor the asked digest (`679e1633…` and `6c3044d5…`). Later work superseded them. Re-run `scripts/tests/zdr-contract-sync.test.sh` to confirm. My run timed out under Git Bash, so this is unverified.
- **Ops:** the Model Plane compose requires `MODEL_QUARRY_TRACE_NATS_PASSWORD`, set on 2026-11-01. `model-nats` picks it up on its next recreate.
- **W5 impact: none blocking.** No W5 slice needs a Model Plane change. The sibling Model Plane program (EP-xx on `model-plane` / `mp-ep14-s1-gap`) is separate and outside this wave.

## 5. Next actions

1. Owner: answer D-W5-1, the identity string. This unblocks W5-S3.
2. Cut W5-S1, W5-S2, W5-S4 and W5-S6 from `main` and run them in parallel. Their files do not overlap: gateway+SPA / quarry-browser+edge auth / profile_routes+SPA dialog / docs. W5-S1 and W5-S4 both touch the SPA, so merge W5-S1 first.
3. Confirm WS9 is on `main` before opening W5-S5.
4. Each slice reports built and proven separately, with the test command and its counts, in `.plan-work/quarry-w1w2/W5-<slice>-progress.md`. This file is updated from those reports.
