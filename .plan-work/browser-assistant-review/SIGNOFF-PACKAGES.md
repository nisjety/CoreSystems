# D1–D8 sign-off packages — agent-first browser (started this turn, awaiting owners)

Each package states: **proposal** (ADR §0.x), **shipped evidence** (commits + verified runs), **still needed** (below the line), **owner decision required**. Status for all: **proposed (not ratified)** per `docs/architecture/adr-agent-browser-phase0-decisions.md` at `487627f60`.

## D1 — Slice 3 permission catalogue (Control) — REPORT owner decision #2
- **Proposal:** Keep `none/read/act`; add `download`/`upload`/`record` as org-restrictible defaults + per-site exceptions; broker + Quarry enforcement, never SPA-only.
- **Shipped:** Capability overview in `BrowserSitesSection` (`6adba79ab`); correctly labels browsing user-settable, downloads ask-every-time, uploads not allowed, recording never, profile ask. No backend enforcement claimed.
- **Still needed:** org-core migration, broker gate, Quarry gate; default-ask consent decision (REPORT §9 #2); TTL decision (#3); budget source decision (#4).
- **Decision required:** Control catalogue + org-core migration owner. Build below the line blocked.

## D2 — Slice 9 secret custody ADR-2 (Model/Ingestion) — REPORT owner decision (custody)
- **Proposal:** Secrets in custody only; model sees `<secret>key</secret>`; Quarry `fill_secret`; receipts booleans only.
- **Shipped:** SPA defence-in-depth `sensitive-redact.ts` (`maybeRedact`) in `CoreShell.submitAssistant` behind `VITE_ASSISTANT_REDACT` (`b0bb61f82`); re-verified this turn (vitest slice modules 28 ✓).
- **Still needed:** `fill_secret` carried by Quarry; custody store; receipt boolean contract test.
- **Decision required:** ADR-2. `fill_secret` below the line blocked (PolicyAsk=off default-deny untouched).

## D3 — Slice 8 single editor R-27 + WS3.K (Model Plane) — REPORT F-t2-034
- **Proposal:** Planner = numbered element table, closed op set; wait constants (combobox 200 ms / 2 frames) as tuning data, not new gate.
- **Shipped:** SPA interactive-only filter `VITE_BROWSER_ELEMENT_FILTER` over existing `DomNodeList` (`a41cb4273`).
- **Still needed:** Planner-side numbered table, `query`-by-text/role lookup, action read-back verification, WS3.K tuning data.
- **Decision required:** R-27 single-editor decision.

## D4 — Slice 2 navigation audit subject (session-core) — REPORT F-t2-036 / F-t2-014
- **Proposal:** session-core owns `verevon.audit.v2.model.*` for navigation runs; frontend gateway never publishes.
- **Shipped:** Durable run: model-gateway `POST /v1/ai/assistant-navigation` → Session Core managed run, `run_id`/`thread_id` returned, GA classification via GatewayDirect (`a4578c505`); re-verified this turn (cargo 2 ✓, gateway `domains::ai` 13 ✓). Cross-plane audit event itself recorded as pending in PROGRESS.md.
- **Still needed:** audit-publisher integration, nav trace retention under ZDR (REPORT §9 #5), target-subject ownership for `verevon.audit.v2.model.*`.
- **Decision required:** Model Plane / session-core owner.

## D5 — Slice 4 annotation attachment (WS9/WS11) — REPORT owner decision #6 (part)
- **Proposal:** Annotation = user comment on viewport coords → governed planner attachment; page region ephemeral; ZDR: user-authored comments only.
- **Shipped:** `AssistantLiveBrowser` Annotate mode behind `VITE_BROWSER_ANNOTATE` (click point + comment, clipboard copy, no persistence) (`aea6f425c`).
- **Still needed:** Attachment contract (WS9/WS11), planner wiring, retention under ZDR.
- **Decision required:** attachment contract before planner wiring.

## D6 — Slice 5 server history retention (Data/Ingestion) — REPORT F-t2-068 / owner decision #6
- **Proposal:** WS8.B metadata receipts are the store; ZDR keeps no entries; purge via `clearHomeLocalContent`.
- **Shipped:** Local bookmarks `verevon.browser-bookmarks.v1` + star/bar behind `VITE_BROWSER_BOOKMARKS`, ZDR-purged (`6d7e08478`); re-verified this turn.
- **Still needed:** Server log + retention design + source filter (agent/user) + clear-data dialog + per-page permission prompts.
- **Decision required:** retention design before server log.

## D7 — Slice 10 replay semantics (WS10/WS8) — REPORT WS10
- **Proposal:** Zero model calls per replayed step; per-step approval; fail closed at first mismatch.
- **Shipped:** `replay-run.ts` `buildReplay` substitutes `{{vars}}`, fails closed on mismatch/missing var/unsafe URL (`1e129b953`); re-verified this turn.
- **Still needed:** Step-picker UI wired to WS10 trace; per-step approval surface.
- **Decision required:** replay semantics before UI.

## D8 — Agent session lifetime (Control + Model)
- **Proposal:** One long-lived Quarry session per (org, user); per-task 5-min grants; new run per task; no shared page state in model context. SPA-side reuse only same-org/same-user/same-origin retained session, closing otherwise.
- **Shipped:** Nothing beyond the proposal — `VITE_AGENT_BROWSER_WORKSPACE`/`VITE_AGENT_BROWSER_LIVE_WS` were orphan env declarations with no implementation and were removed this turn (re-add only when code lands). Phase 1/2 proceed on the conservative SPA side.
- **Still needed:** cross-plane session store + isolation semantics + WS frame transport for live view.
- **Decision required:** Control + Model owners.

## SC1–SC7 evidence ledger (non-container portion run this turn)

- **SC1 compose smoke:** NOT run — requires container/live changes, blocked until approval. Records: compose files verified by read only.
- **SC2 ZDR refusal:** Unit: `assistant_task_session_is_refused_under_zdr_before_any_grant` included in `domains::browser` run (218 ✓, includes new ZDR test). Live (409 before grant under ZDR org on a live stack): NOT run.
- **SC3 navigation persistence:** Unit: model-gateway `assistant_navigation` 2 ✓ + gateway `domains::ai` 13 ✓ re-verified this turn. Live run-id round-trip: NOT run.
- **SC4 bookmarks purge:** Unit: `browser-bookmarks` suite within slice-modules 28 ✓. Live purge via `clearHomeLocalContent`: NOT run.
- **SC5 PDF content-type:** Unit: `only_safe_browser_artifact_content_types_are_forwarded` + `model_screenshot_content_types_are_image_only` 2 ✓ re-verified this turn (incl. `%PDF-` cases from `dfe8bb618`). Live artifact fetch: NOT run.
- **SC6 live view + shell:** Unit: `BrowserChrome.reload` 7 ✓ re-verified this turn (previously reported stale-heading failure no longer reproduces — see PROGRESS.md). Live dock/tab/annotate/split/floating render: NOT run.
- **SC7 flag-off parity + triage:** Unit: `domains::browser` 218 ✓ / 3 ✗ (see triage below); `BrowserChrome.reload` now 7 ✓. Pre-existing heading failure resolved on current HEAD. Live flag-off parity: NOT run.

## Triage — 3 `domains::browser` failures (pre-existing on clean HEAD, unrelated to slices)

- **Tests:** `a_real_grant_run_starts_under_the_issued_grant_with_the_browser_bearer`, `a_real_grant_run_with_no_host_to_grant_is_refused_before_any_browser_mint`, `a_refused_grant_is_answered_as_is_and_no_run_starts` — all get `409` instead of `200/400/403`.
- **Verified by running this turn:** full `domains::browser` 218 passed / 3 failed (53 s run). Our branch diff `3bb5c96db..HEAD` on `browser.rs` (+56 lines) does **not** touch `ai_run_upstreams`, `test_app_state`, org mocks, or `effective_zdr` (proven by diff).
- **Root cause (read + diff, not guessed):** `ai_run_upstreams` sets Quarry/auth/model URLs but never mocks org-core, so `test_app_state`'s unreachable `org_core_url` (`127.0.0.1:1`) makes `org_zdr_enabled` fail closed to `true` (`Unknown` → assume ZDR, `zdr.rs`). Then `effective_zdr = metadata.zdr() || org_zdr_enabled` is `true`, the real-grant arm `None if real_grant_switch && !effective_zdr` is skipped, and the handler returns `browser_session_grant_missing` (409). At `34bb816db` (where the tests were green per commit message) `start_ai_run` had no `|| org_zdr_enabled` check; the later Space/ZDR-resolver merge added the check without updating these mocks. Same helpers and URLs verified identical at `3bb5c96db`.
- **Fix (not applied — no-code-change rule):** extend `ai_run_upstreams` to mock `GET /api/v1/organizations/{orgId}` returning `{"data":{"metadata":{"interactiveRetention":{"zdr":false}}}}` (and route `state.org_core_url` to that mock) for the `zdr=false` cases. Test-helper only; no product-behaviour change. Owner/merge gate must accept the fix or explicitly waive these 3 tests — merge cannot proceed with them red.
- **Not a security/ZDR leak:** the fail-closed direction is correct (assume ZDR on outage). No confirmed leak requiring a fix behind no flag; tracks REPORT.md's "unverified candidate leaks" rule.

## `BrowserChrome.reload.test.tsx` stale heading

- **Verified by running this turn:** 7 passed, 0 failed (16 s). The previously reported stale-heading failure no longer reproduces on `487627f60`. Recorded as resolved on current HEAD; SC7 live parity still pending.
