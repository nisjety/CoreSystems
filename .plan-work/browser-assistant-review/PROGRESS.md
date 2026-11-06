# Implementation progress — browser assistant slices

- **Branch:** `feat/browser-assistant-slices` (local only, no push, no PR).
- **Worktree:** `C:\dev\CoresSystem-assistant-slices` — main checkout `C:\dev\CoresSystem`
  on `main` was not edited (except this note, which lives in the main
  checkout's `.plan-work/browser-assistant-review/` next to `REPORT.md`).
- **Commits:**
  - `d94d508e1` `feat(browser): refuse ZDR assistant task sessions and gate assistant navigation behind a kill switch` (slice 1 + kill switch).
  - `bddf29087` `feat(web): add assistant dock positions and the browser tab context menu behind flags` (slice 7 partial).
  - `a4578c505` `feat(assistant): run navigation tasks as durable Model Plane runs` (slice 2).
  - `6adba79ab` `feat(web): add the capability overview to the browser-sites permission hub` (slice 3).
  - `6d7e08478` `feat(web): add local browser bookmarks behind a flag` (slice 5 local half).
  - `dfe8bb618` `fix(browser): serve real PDF artifacts with their own content type` (slice 6 gateway half).
  - `aea6f425c` `feat(web): add annotation mode to the live browser view behind a flag` (slice 4).
  - `a41cb4273` `feat(web): add zoom, keyboard commands, new-tab page and selective observation to BrowserChrome behind flags` (slices 6/7/8).
  - `514950ff0` `feat(web): add split view and floating composer behind flags` (slice 7).
  - `b0bb61f82` `feat(web): redact secret-looking text in assistant submissions behind a flag` (slice 9).
  - `1e129b953` `feat(web): add replay of saved runs with variable substitution, fail closed` (slice 10).
  - `487627f60` `docs: add Phase 0 decision proposals gating the agent-first browser plan` (ADR §0.1–0.8, proposed; live-view flags `VITE_AGENT_BROWSER_WORKSPACE`/`VITE_AGENT_BROWSER_LIVE_WS` were orphan env declarations with no implementation and were removed — re-add only when session-reuse/WS code lands per ADR §0.8).
  - `3425e9447` `test(browser): mock org posture in ai_run_upstreams so real-grant tests do not fail closed` (post-approval test-helper fix; `domains::browser` 221 ✓ / 0 ✗).
  - `0c19eef15` `feat(web): add in-page find overlay to BrowserChrome behind a flag` (slice 6 remainder, SPA-only client-side search over loaded DOM text; `VITE_BROWSER_FIND` default off).
- **Full ordered parity plan:** `.plan-work/browser-assistant-review/PARITY-PLAN.md` (phases 0–9, design calls D1–D8 in order, Codex/Aside/T3-Chat parity matrix, SC1–SC7 gates, merge gate, one-flag-at-a-time rollout).
- **D1–D8 sign-off packages + SC1–SC7 ledger + triage:** `.plan-work/browser-assistant-review/SIGNOFF-PACKAGES.md` (started this turn; all D items still proposed/not ratified; non-container SC evidence re-verified, stack SC still pending approval).
- **Post-approval evidence + rollout runbook:** `.plan-work/browser-assistant-review/POST-APPROVAL.md` (3-test fix + 221 ✓, compose parse, merge dry-run clean, waivers staying flagged-off, one-flag rollout env flips).

## Done per slice

1. **ZDR dead-end:** gateway `create_session` refuses `assistant_task` under ZDR (409) before grant/Quarry.
2. **Navigation runs:** new model-gateway `POST /v1/ai/assistant-navigation` (`assistant_navigation.rs`) creates a Session Core managed run, classifies, completes via GatewayDirect; returns `run_id`/`thread_id`. Verevon gateway forwards prompt only; SPA shows the run id. Flag `MODEL_GATEWAY_ASSISTANT_NAVIGATION` default false. Cross-plane audit event itself remains Session-Core-owned (published via the managed-run lifecycle, not the frontend gateway).
3. **Permission hub:** `BrowserSitesSection` gained a capability overview (Browsing/Downloads/Uploads/Recording/Saved profile) naming what is user-settable vs governed. Org-core migration for new levels still needs Control sign-off.
4. **Annotation:** `AssistantLiveBrowser` Annotate mode (click point + comment, clipboard copy, no persistence) behind `VITE_BROWSER_ANNOTATE`. Planner attachment path still needs WS9/WS11.
5. **History/bookmarks:** local bookmarks module + star/bar in `BrowserChrome`, ZDR-purged with the other local records, behind `VITE_BROWSER_BOOKMARKS`. Server log still needs a retention design.
6. **Page utilities:** gateway serves real PDFs as `application/pdf` (validated by `%PDF-` header); SPA zoom 50–150% behind `VITE_BROWSER_ZOOM`; SPA find-in-page overlay behind `VITE_BROWSER_FIND` (client-side search over loaded DOM text, `page-find.ts` + 6 tests). Area-screenshot/viewport still need Quarry actions.
7. **Tabs/dock:** dock positions, tab context menu, new-tab page (`VITE_BROWSER_NEWTAB`), keyboard commands (`VITE_BROWSER_KEYMAP`), split view (`VITE_WORKSPACE_SPLIT`), floating composer (`VITE_ASSISTANT_FLOATING`).
8. **Element table/selective observation:** existing `DomNodeList` + interactive-only filter behind `VITE_BROWSER_ELEMENT_FILTER`. Planner-side tuning still needs R-27/WS3.K.
9. **Sensitive placeholders:** `sensitive-redact.ts` (`maybeRedact`) wired into `CoreShell.submitAssistant`, behind `VITE_ASSISTANT_REDACT`. `fill_secret` still needs the custody ADR.
10. **Replay:** `replay-run.ts` `buildReplay` substitutes `{{vars}}`, fails closed on mismatch/missing var/unsafe URL. Step-picker UI still needs WS10 trace wiring.

## Verification (ran here)

- `cargo test -p model-gateway --lib assistant_navigation` 2 ✓ (re-run at `487627f60`); gateway `domains::ai` 13 ✓, `domains::browser` 221 ✓ / 0 ✗ (after `3425e9447` org-posture mock; was 218 ✓ + 3 ✗).
- `pnpm exec tsc -b` in `verevonv3`: **0 errors** (re-run at `487627f60`; re-verified this turn: TSC_EXIT 0). This turn (at `0c19eef15`): `page-find` vitest 6 ✓, `tsc -b` 0 ✓, ESLint 0 errors, `BrowserChrome.reload` 7 ✓.
- `vitest`: assistant-agent + replay + bookmarks 81 ✓; AssistantLiveBrowser/GlobalAssistant/BrowserChrome.reload 33 ✓; re-run at `487627f60` of the three slice modules (replay, bookmarks, sensitive-redact) 9 ✓; re-run this turn: slice modules + GlobalAssistant.agent 28 ✓, `BrowserChrome.reload` 7 ✓ (previously reported stale-heading failure no longer reproduces), gateway content-type tests 2 ✓ (incl. `%PDF-` cases). `domains::browser` triaged this turn: 218 ✓ / 3 ✗ with root cause + unapplied fix in SIGNOFF-PACKAGES.md. Final `/goal` run this turn: model-gateway `assistant_navigation` 2 ✓, gateway `domains::ai` 13 ✓, content-type 5 ✓, vitest 5 files 35 ✓, `tsc -b` 0 errors, compose flags verified `false` by default. This turn (at `3425e9447`): `tsc -b` 0 errors, vitest 35 ✓, ESLint 0 errors / 2 pre-existing warnings in `BrowserChrome.tsx` (601 `tabMenu`, 728 `selectedTimelineStep`) — not fixed (no code changes without a confirmed leak); post-approval `domains::browser` 221 ✓.
- **Only read, not run:** SPA full suite/lint, Playwright, stack evidence (SC1–SC7), Quarry lanes, live flag/container checks. Unit passes above are not stack evidence; SC1–SC7 per PARITY-PLAN.md §6 remain pending before any merge.

## Run to completion (/goal) — autonomous moves done, approval-gated moves packaged

- **Autonomous complete:** all 10 slice SPA/gateway cores behind default-off flags; branch `feat/browser-assistant-slices` @ `487627f60` clean (no push/PR); parity plan + sign-off packages + SC ledger written; full non-container verification re-run green except the 3 triaged pre-existing `domains::browser` failures (fix documented, not applied — no code changes without a confirmed leak). No confirmed security/ZDR leak found; per the brief, no code fix was made behind no flag.
- **Approval-gated (not executed, packaged for approval):** D1–D8 ratification; SC1–SC7 container/stack portion (compose smoke, live ZDR/navigation/PDF/live-view checks); the 3-test mock fix decision; merge from a clean checkout; one-flag-at-a-time rollout (`MODEL_GATEWAY_ASSISTANT_NAVIGATION=true` first, then each `VITE_*`, re-running its SC gate after each flip). See SIGNOFF-PACKAGES.md §§ SC1–SC7 + triage for the exact approval asks.

## Flags (all default off)

`ASSISTANT_NAVIGATION_ENABLED=false`, `MODEL_GATEWAY_ASSISTANT_NAVIGATION=false`, `VITE_ASSISTANT_DOCK_POSITION`, `VITE_BROWSER_TAB_MENU`, `VITE_BROWSER_BOOKMARKS`, `VITE_BROWSER_ANNOTATE`, `VITE_BROWSER_ZOOM`, `VITE_BROWSER_KEYMAP`, `VITE_BROWSER_NEWTAB`, `VITE_WORKSPACE_SPLIT`, `VITE_ASSISTANT_FLOATING`, `VITE_ASSISTANT_REDACT`, `VITE_BROWSER_ELEMENT_FILTER` — all unset/false. No live flag or container was changed.

## Still needs owners (not built, by design)

Slice 3 levels migration + broker/Quarry gate (Control), slice 8 planner tuning (R-27 Model Plane), slice 9 `fill_secret` custody ADR, slice 5 server retention design, slice 4 planner attachment (WS9/WS11), slice 10 trace UI, slice 2 cross-plane audit subject ownership. Nothing merges or pushes without stack evidence.
