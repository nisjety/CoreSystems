# Quarry Wave 5 orchestration, pass 2 (status, ADR-5 fence, Model Plane, next slice)

Date: 2026-11-08. Worktree `vigorous-perlman-e7ae5b`, branch `quarryv2-next` at `f55beef14`. `main` is at `2a776c837` (2026-11-06).
This file follows `W5-ORCH.md` (2026-11-07) and supersedes its sections 2, 3 and 4. Inputs: `W5-ORCH.md`, `W5-S1-progress.md`, `W5-S2-progress.md`, `W5-S6-progress.md`, `W5-VERIFY.md`, plus `git log`, `git grep` and `git show` against `main`, `model-plane`, `quarryv2` and `w23/integ`.

Rules: notes only, local commits only. Nothing here was run on a live stack, and I ran no tests in this pass. "Built" means the code is in a branch. "Proven" means a test or a live check shows it works. A row is done only when it is proven.

## 0. What changed since W5-ORCH

1. **`main` already holds W5 work that W5-ORCH missed.** On 2026-11-05 and 2026-11-06, outside the W5 slice process, with no progress files, all behind default-off flags:
   - `bddf29087`: the tab context menu (F-t2-070), behind `VITE_BROWSER_TAB_MENU`.
   - `a41cb4273`: zoom and find (F-t2-069 presentation), behind `VITE_BROWSER_ZOOM` and `VITE_BROWSER_FIND`, and keyboard commands (F-t2-070), behind `VITE_BROWSER_KEYMAP`.
   - `6d7e08478`: local bookmarks (F-t2-068 local slice), behind `VITE_BROWSER_BOOKMARKS`.
   - `dfe8bb618`: the gateway serves a browser artifact as `application/pdf` only when the bytes start with `%PDF-`. This is the content-type half of 069 step 1, and it has a unit test.
   - `6b538f324`: the Quarry tab routes, including `POST /v1/agent/runs/:run_id/tabs/:tab_id/mark` (`agent_routes/router.rs:43-60`). This closes MP-ASKS (e), "tab-mark route not built", in code.
2. **WS9 is on `main`.** WS9.G and WS9.H came in through `w23/integ`, and the gateway proxies the tab routes (`browser.rs:535-543`). So the W5-S5 gate ("confirm WS9 on `main`") is met.
3. **`quarryv2-next` now has 5 commits that `main` lacks:** `9fa3d7993`, `e661fc41f`, `f90ed49a3` (074 provider half), `79843694c` (071a) and `f55beef14`. `main` is 1287 commits ahead. Getting the two code commits into `main` is the owner's call.
4. **One new ADR-5 fence finding, on `main`** (section 1, FENCE-1).

## 1. ADR-5 fence

Option (a), honest identity, is decided. W5-ORCH section 1 lists the five guard rules, and they still hold.

**Sweep on 2026-11-08.** `git grep` over `apps/**/*.{rs,go,ts,tsx,js,mjs,cjs}` on `main`, `model-plane`, `quarryv2` and `w23/integ`, for UA overrides, `navigator.webdriver`, stealth plugins, `FingerprintRotator`, CAPTCHA solvers and consent clicks.

| # | Finding | Where | Rule | Severity | Action |
|---|---|---|---|---|---|
| FENCE-1 | **Automatic cookie-banner dismissal in the desktop browser agent.** After navigation, the adapter calls `dismissOptionalCookies` by itself. The Model never proposes this click. It finds consent iframes by title (`cookie\|informasjonskapsl\|samtykke\|consent`). It clicks "reject all" or "only necessary". If neither exists, it clicks "customise or reject". It never clicks accept. There is no org policy key, no flag, and no receipt or trace entry for the click. | `main` `5e1c682d3` (2026-11-06): `apps/Ingestion Plane/Quarry-v2/adapters/desktop-browser/index.mjs:246-300` and `page.mjs:41-62`. The code is also on `model-plane`, `quarryv2` and `w23/integ` | Rule 4 (no consent automation; F-t2-066 parked) and D-W5-3. It is also against the plane rule that the Model proposes and Quarry executes or rejects | Medium. It grants no tracking consent, so the user harm is low. But it ships without the ADR-5 amendment that D-W5-3 requires: reject-first only, behind an org policy key, every click in the receipts | **Owner decision D-W5-4** (below). Not mine to edit: another writer's file on `main` |
| FENCE-2 | A stale snapshot of an old worktree is committed inside Quarry: `apps/Ingestion Plane/Quarry-v2/.claude/worktrees/vigilant-banach-6933aa/`, 304 files, added in `1eecf8d0c` on 2026-05-30. It holds the old `quarry-tls` `user_agent_override` and "browser-family fingerprint to impersonate" in `tls_driver.rs`. | All four branches, including `quarryv2-next` | None live. It is not a workspace member (`Quarry-v2/Cargo.toml` lists `crates/*` only) | Low, but it is noise in every ADR-5 sweep, and someone could copy from it | Delete it in a hygiene slice. That is 304 deleted files, so it needs the owner's OK |
| ok | Desktop agent identity: no UA override (no `setUserAgent` or `userAgent` hits), so it uses the stock Electron UA. Each run gets a new partition, `quarry-desktop-<uuid>` with `cache:false` (`desktop-browser/main.mjs:52`), so the user's own Chrome profile is not used and 077 is not breached. The cursor is a visible overlay in a closed shadow root that dispatches only real CDP input, not a disguise. | `main` | 1, 2, 077 | none | Record only |
| ok | `challenge.rs` change in `292199ca1`. `main_document_status` now requires `topLevelDocument` and the same tab. This is classification only. | `main` | 5 | none | none |
| watch | Model Plane `mp-w4c` C1 typed computer connector (`5fbbe2c79`, `model-plane` only). It adds `computer.observe` and `computer.act`, and actions reference elements, not coordinates. There is no backend, the flag `EXECUTION_DO_COMPUTER_ENABLED` is false, and ZDR runs are refused. The browser-assistant PARITY-PLAN rejects desktop CUA under ADR-5/6/7. | `model-plane` | 077 / ADR-6 | none today | Any backend for it needs an ADR before it merges. Flag this to the Model Plane lead |

Otherwise clean. The `browserSiteConsentChoices` hits are a site-permission prompt for the user, not cookie consent. Every `stealth` hit is a refusal, a test, or the deprecated preset alias (as in W5-VERIFY).

**Owner decisions (ADR-5), current:**
- **D-W5-1 (blocks W5-S3), still open.** Pick the one honest identity string. The five strings are unchanged on `main`:
  - `config.rs:341` and `answer.rs:892`: `Quarry/2.0 (+https://triodelab.com/quarry)`
  - `policy.rs:112`: `Quarry/2.0`
  - `main.rs:1744`: `Mozilla/5.0 (compatible; QuarryBot/1.0; +https://verevon.io/bot)`
  - `fetch.rs:22`: `QuarryBot`

  `robots_gate.rs:51` already expects a one-line change once the string is chosen. Recommended: `QuarryBot/2.0 (+https://verevon.io/bot)`.
- **D-W5-2, not blocking.** The per-tier UA stance. Add the desktop Electron tier to the question: it shows a stock Electron UA with no product token.
- **D-W5-3, parked.** The 066 consent preset. FENCE-1 means this is no longer only theoretical.
- **D-W5-4 (new, from FENCE-1).** Choose one:
  - (a) Amend ADR-5 to allow reject-only dismissal and add the controls D-W5-3 named: an org policy key (default off), a receipt or trace line for each dismissal, and no "customise" fallback click.
  - (b) Remove the auto-dismiss, and let the Model propose the click or the user do it.

  Recommended: (a) without the "customise" fallback, because a reject-only click leaves the user's privacy no worse.
- **D-W5-5 (new, from W5-S1).** How to serve a PDF artifact. `main` serves it inline on the app origin with `nosniff`, a `%PDF-` magic check, and no `Content-Disposition` or CSP (`browser.rs` around line 2690). Recommended: add `Content-Security-Policy: sandbox` to `application/pdf` responses. This is the small part of W5-S1b.

## 2. Slice board

| Slice | Items | Built on `quarryv2-next` | Built on `main` | Proven | Status / next step |
|---|---|---|---|---|---|
| W5-S1 | 071a (delete WebPageViewer); 069 step 1 | 071a yes (`79843694c`) | 071a no (`DashboardSearchResults.tsx:147` still there). 069: PDF content type yes (`dfe8bb618`); `sanitize_action` still rejects `pdf` (`browser.rs:5191`), and `screenshot` passes with no `full_page` check (`browser.rs:5128`) | 071a: tests only (vitest, 665 pass, from S1-progress). PDF content type: a unit test exists in `dfe8bb618`, not run by me | **PARTIAL. Next: W5-S1b** (section 4) |
| W5-S2 | 074 provider errors; JWKS stale-if-error; RUNBOOKS dependency matrix | provider half yes (`f90ed49a3`) | no (no `stale_if_error` in `quarry-edge/src/auth.rs`) | provider half: tests only (`quarry-browser --lib`, 210 pass, from S2-progress) | PARTIAL. JWKS needs its own security review: it must not extend a revoked key |
| W5-S3 | 067 slice 1, one UA constant | no | no | no | BLOCKED on D-W5-1 |
| W5-S4 | 068 clear-data for the stored `SessionSnapshot` | no | no (no clear route in `profile_routes.rs`). The local bookmarks in `6d7e08478` are a different 068 item | no | OPEN, nothing blocks it |
| W5-S5 | 070 tab-strip context menu | no | yes, behind `VITE_BROWSER_TAB_MENU` (`BrowserChrome.tsx:1467`, `bddf29087`), plus the keymap (`a41cb4273`). The gateway and Quarry tab routes exist | no. The menu commit adds no `BrowserChrome` test for the menu (it changes `GlobalAssistant.agent.test.tsx` only). The flag is off by default | **RE-SCOPE to verify-and-close.** Check it against the slice spec: an action contract in `src/shared/actions` (repo rule; only `home-tab.ts` exists), menu tests, and a flag-on walk |
| W5-S6 | 075 and 077 notes; stale-doc rows | notes yes (`f55beef14`, in `W5-S6-progress.md`) | no | n/a (docs) | PARTIAL. Still to do: the stale-doc rows for `FIXES_APPLIED.md` (84, 92), `SCRAPING_ENGINE_GAPS.md` (19, 136-204) and `GAP.md` (`blockAds`) |

Off-plan W5-adjacent work on `main`, none of it a W5 slice: 069 zoom and find, 068 bookmarks, and the desktop browser agent. Record it, schedule nothing.

## 3. Built vs proven (effort-weighted)

Weights come from W5-ORCH: S1 1 d, S2 3 d, S3 2 d, S4 5 d, S5 6 d, S6 1 d, about 18 d in total. A slice share is my estimate of how much of its spec is in the tree.

| Slice | Weight | Built share, both branches | Built days |
|---|---|---|---|
| S1 | 1 | 0.6 (071a and the PDF content type; the action allowlist is missing) | 0.6 |
| S2 | 3 | 0.5 (provider half) | 1.5 |
| S3 | 2 | 0 | 0 |
| S4 | 5 | 0 | 0 |
| S5 | 6 | 0.6 (menu and routes built; contract, tests and verification missing) | 3.6 |
| S6 | 1 | 0.6 | 0.6 |
| **Total** | **18** | | **6.3** |

- **Built: about 35 %** across `main` and `quarryv2-next` (6.3 of 18 days). It is **about 15 %** if only the work done inside the W5 slice process on `quarryv2-next` counts (2.6 days). W5-ORCH's "5 %" was too low because it missed the `main` work.
- **Proven:**
  - Live: **0 %**.
  - Tests only: 071a, the 074 provider half, and the PDF content type. That is about 10 % of the effort.
- **Slices done: 0 of 6.** None is proven end to end.

## 4. Model Plane status (2026-11-08)

- **`main`, through 2026-11-06.**
  - The `mp-w2w3` program (W2 30 rows, tested in process; W3 13 rows, compile-checked) is merged, as W5-ORCH said.
  - Since then, the W3-22 to W3-25 wave batches have landed (2026-11-05). They include:
    - EP-12 API runs and agent API routes
    - EP-4-S10 app-initiated tools/call
    - EP-9-E3 sandbox network boundary
    - EP-11-S8/S9
    - EP-12-S14/S15
  - Also on `main`: `a4578c505`, assistant navigation as durable Model Plane runs, and `5e1c682d3`, `model-gateway` `desktop_browser_planner.rs`, which plans for the desktop agent.
  - Proven live: nothing new. Every item is behind a default-off flag, according to the commit messages.
- **`model-plane` branch (`5fbbe2c79`, 2026-11-07): 28 commits ahead of `main` and not merged.** They include:
  - EP-01 browser takeover and give-back
  - EP-02 web reading
  - EP-03 artifact revisions
  - EP-04 inline widgets
  - EP-05 decks
  - EP-06 automations
  - EP-07 persistent goals
  - EP-09 egress tiers (sandbox only, no rotation on block)
  - EP-10 connector suggestion
  - EP-11 versioned skills
  - EP-12 export
  - EP-14 turn folding
  - `mp-w4a` to `mp-w4f` (replay scrubber, viewer registry, the C1 computer connector, M1 policy and CSP, the T1 trigger record)
  - `mp-w5-069/078/079/086`

  **Naming clash:** "mp-w5-0xx" is the Model Plane's own wave. It is not Quarry W5. Its ids match F-t2 numbers only by chance (069 there is "rewind side chat"). Do not count it here.
  - Commit messages claim unit and contract tests. I did not run them.
  - `mp-ep14-s1-gap` (`a480f985d`) is 2 commits ahead of `main` and also unmerged.
- **MP-ASKS, from W5-ORCH:**
  - (a) and (c), the golden re-vendors: unverified, not re-run.
  - (b) the broker `RenewGrant` decision: still open.
  - (d) the `api_key_revoked` transport: still open.
  - (e) the tab-mark route: built on `main` in `6b538f324`, not proven.
- **W5 impact.** No W5 slice needs a Model Plane change. Two watch items for the Model Plane lead:
  1. The C1 computer connector must not get a backend without an ADR.
  2. `desktop_browser_planner.rs` drives the adapter that carries FENCE-1. If D-W5-4 is (b), the planner must offer "dismiss banner" as a proposable action.

## 5. Next slice

**W5-S1b: finish 069 step 1 in the gateway. Cut it from `main`.** It is less than 1 day, Frontend lane, and needs no decision beyond D-W5-5, which has a safe default.
- File: `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/browser.rs` on `main`.
- `sanitize_action` (line 5108):
  - Add a `"pdf"` arm that accepts only `{type}` and rejects extra keys (the same pattern as `reload`).
  - Split `"screenshot"` out of the pass-through arm (line 5128). Accept only `type` and an optional boolean `full_page`, and reject everything else.
- Artifact response (around line 2690): when `content_type` is `application/pdf`, also send `Content-Security-Policy: sandbox`.
- Tests:
  - `sanitize_action_allows_pdf_without_extra_keys`
  - `sanitize_action_rejects_pdf_with_extra_keys`
  - `sanitize_action_allows_screenshot_full_page_bool`
  - `sanitize_action_rejects_screenshot_full_page_non_bool`
  - `sanitize_action_rejects_screenshot_unknown_keys`
  - `pdf_artifact_response_is_sandboxed`
- Command: `cd "apps/Frontend Plane/verevonv3/apps/gateway" && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test sanitize_action browser_artifact`. Expect a cold build.
- Report in `W5-S1-progress.md` (append). Note that 071a is still to be ported to `main`.

**In parallel (no file overlap):**
1. **W5-S4** (068 clear-data, about 5 days, Ingestion + Frontend, from `main`). It is the largest open slice and nothing blocks it.
2. **W5-S5 verify-and-close** (about 1 day). Write the missing action contract and the menu tests, then do a flag-on walk.

**Owner, in this order:**
1. D-W5-4 (FENCE-1)
2. D-W5-1 (it unblocks W5-S3)
3. Get `f90ed49a3` and `79843694c` into `main`
4. Approve the FENCE-2 deletion
