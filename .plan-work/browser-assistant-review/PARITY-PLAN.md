# Governed agent-first browser — full ordered parity plan

**Goal:** Build a governed, agent-first browser inside Verevon with live real-time view, at 100% parity with Codex / Aside / T3-Chat agent browsers, following the full ordered plan from start to completion.

**Current verdict (from REPORT.md): Conditional pass, do not GA.** The governed core (WS1/WS7/WS8) is intact; the gaps are scope and cross-plane contracts, not architecture violations. Every new behaviour lands behind a flag defaulting off. Nothing merges or pushes until SC1–SC7 evidence is recorded and owner sign-offs land.

**What I verified by running (this turn):** `git status` / `git log` in `C:\dev\CoresSystem-assistant-slices` (branch `feat/browser-assistant-slices`, 12 commits `d94d508e1` → `1e129b953` plus the PDF fix `dfe8bb618`); `Select-String` in `src/` proving the two `VITE_AGENT_BROWSER_*` flags were orphan declarations with no code; `read` of REPORT.md, PROGRESS.md, Model Plane PLAN.md, ADR phase-0 proposals. **What I only read:** all plane source, contracts, audit and plan documents listed in REPORT.md §1; no tests, builds, containers or stack were run this turn.

**Rules that apply to every phase below (from the review brief):**
- Models: Sonnet 5.5 high for build; Opus 5.5 only for grant/security design (C).
- Local commits only, no push, no PR, never `--no-verify`.
- No edits to uncommitted files in `C:\dev\CoresSystem` or `C:\dev\verevon-audit-2026-10-14`; no merge/push/live-flag/container changes until evidence + approval.
- New behaviour behind a flag defaulting off; state plainly what was verified by running vs only read.
- Deliverable: `.plan-work/browser-assistant-review/REPORT.md` (done) + this plan + PROGRESS.md updates.
- No code changes unless a security/ZDR leak is confirmed — then fix behind no flag with a test, listed first. No confirmed leak was found in the review; the two candidate leaks remain unverified in REPORT.md §3 and must not be "fixed" without a reproduction.

---

## 1. What "100% parity" means here

Parity is **governed parity**, not feature-for-feature cloning. Anything that requires desktop control (OS-level CUA), model-written JS, stealth/anti-bot, profile import, or MCP exposure is rejected for all three references (ADR-5/ADR-6/ADR-7). The matrix in §5 marks each reference feature as **Adopt** (map to a governed Verevon slice) or **Reject** (with the ADR that rejects it).

Live real-time view means: the user watches the agent's cloud browser session in-page (retained session + attachment), with human take-over (WS2/WS8), stop with confirmation, and a 3 s replay poll today; WebSocket frame transport is the remaining upgrade (§4, Phase 2).

---

## 2. Architecture invariants (non-negotiable)

1. **Model proposes, Quarry executes or rejects.** The SPA never builds a selector or script; the model never receives selectors/JS. Closed declarative lists only (ADR-6).
2. **Site policy is server-side in three layers** — org-core `explain`, broker `assistant_task` always-consult, gateway per-hop redirect walk. The SPA only renders outcomes (WS7).
3. **ZDR propagates** through every content-persisting boundary; ZDR orgs get no history, no attachments, no persisted grants (RUNBOOKS.md).
4. **Budgets apply** (WS6/W6.E); caps come from a single ledger shared by navigation and browser paths once slice 2 lands.
5. **One canonical trace** (F-t2-014/F-t2-036): every assistant task — navigation or browser — has a run record, thread binding, and audit event.
6. **No direct database crossing between planes; no independent embeddings/reranking outside isolated labs.** Model may propose browser actions; Quarry-v2 executes or rejects them.

---

## 3. Design calls in required order (cross-plane owners)

Execute in this order — a later call cannot start its build until the earlier call's owner decision is recorded in the ADR (`docs/architecture/adr-agent-browser-phase0-decisions.md` §0.1–0.8, status: proposed):

| Order | Call | Owner | Gate |
|---|---|---|---|
| D1 (§0.1) | Slice 3 permission catalogue: add `download`/`upload`/`record` levels with per-site exceptions; broker + Quarry enforcement, never SPA-only | **Control** (catalogue + org-core migration) | Blocks slice 3 below the SPA hub |
| D2 (§0.2) | Slice 9 secret custody ADR-2: secrets in custody only; model sees `<secret>key</secret>`; Quarry `fill_secret`; receipts carry booleans | **Model/Ingestion** | Blocks `fill_secret` build |
| D3 (§0.3) | Slice 8 single-editor rule R-27 + WS3.K tuning (combobox 200 ms / 2 frames as data, not new gate) | **Model Plane** | Blocks planner-side selective observation |
| D4 (§0.4) | Slice 2 navigation audit subject: session-core owns `verevon.audit.v2.model.*`; frontend gateway never publishes | **Model Plane / session-core** | Blocks audit-publisher integration |
| D5 (§0.5) | Slice 4 annotation attachment contract (WS9/WS11): user comment on viewport coords, governed planner attachment; page region ephemeral | **Model Plane** | Blocks planner wiring of annotation |
| D6 (§0.6) | Slice 5 server history retention: WS8.B metadata receipts are the store; ZDR keeps no entries; purge via `clearHomeLocalContent` | **Data/Ingestion** | Blocks server log build |
| D7 (§0.7) | Slice 10 replay semantics (WS10/WS8): zero model calls per replayed step; per-step approval; fail closed | **Model Plane** | Blocks step-picker UI |
| D8 (§0.8) | Agent session lifetime: one long-lived Quarry session per (org, user); per-task 5-min grants; new run per task; no shared page state in model context. SPA-side reuse is same-org/same-user/same-origin retained session only, closing otherwise | **Control + Model** | Blocks cross-plane session reuse; Phase 1/2 proceed on the conservative SPA side only |

**Owner decisions still open from REPORT.md §9:** (1) ZDR ship vs refuse — decided: refuse (slice 1); (2) default-ask consent per site — open, must surface in hub before GA; (3) `assistant_task` TTL 5 min vs 15-min session grant — open; (4) budget source (start-body caps vs WS6.E grant claim) — open; (5) nav trace retention under ZDR — open with D4; (6) annotation/history retention under ZDR — open with D5/D6; (7) rename/document `assistant_interactive` — open.

---

## 4. Phases — ordered from start to completion

### Phase 0 — Governance & decision gates (in progress, blocks everything below the line)
- Collect D1–D8 sign-offs in the ADR; no cross-plane semantics change until each is signed.
- All 13 flags below stay default-off (`ASSISTANT_NAVIGATION_ENABLED=false`, `MODEL_GATEWAY_ASSISTANT_NAVIGATION=false`, all `VITE_*` unset).
- **Exit gate:** ADR status moves from *proposed* to *ratified* per section, or phases sit idle behind flags.

### Phase 1 — ZDR & kill-switch safety (done — slices 1 + kill switch)
- Slice 1: gateway `create_session` refuses `assistant_task` under ZDR (409) before grant/Quarry (`d94d508e1`). Contract test included.
- Kill switch: `ASSISTANT_NAVIGATION_ENABLED` in gateway `ai.rs`; model-gateway route refuses unless `MODEL_GATEWAY_ASSISTANT_NAVIGATION=true` (`a4578c505`; `switch_from` is case-insensitive `true` only — `1`/`FALSE` rejected).
- **Evidence:** SC2 (ZDR refusal), SC7 (flag-off parity). Owner decision recorded: ZDR assistant ships refused, not as a ZDR attach lane.

### Phase 2 — Live real-time view & assistant shell (shipped behind flags; WS transport pending)
- Live view: `AssistantLiveBrowser` (retained session, approval events, 3 s replay poll, stop with confirmation, take-over via `human_takeover` epoch). Annotation mode behind `VITE_BROWSER_ANNOTATE` (`aea6f425c`, slice 4 SPA draft: click point + comment, clipboard copy, no persistence).
- Shell: dock positions `VITE_ASSISTANT_DOCK_POSITION` + tab context menu `VITE_BROWSER_TAB_MENU` (`bddf29087`), split view `VITE_WORKSPACE_SPLIT` + floating composer `VITE_ASSISTANT_FLOATING` (`514950ff0`), new-tab page `VITE_BROWSER_NEWTAB` + keyboard commands `VITE_BROWSER_KEYMAP` (`a41cb4273`).
- **Pending:** WS frame transport for the live view (`VITE_AGENT_BROWSER_LIVE_WS` was declared then removed this turn — no implementation existed; re-add only when the transport lands). Session-reuse across tasks stays SPA-local (same org/user/origin) pending D8.
- **Evidence:** SC6 (live view + dock/tab/annotate), SC7.

### Phase 3 — Durable runs & trace parity (slice 2 done, audit pending D4)
- Slice 2: navigation as a durable Model Plane run — new model-gateway `POST /v1/ai/assistant-navigation` (`assistant_navigation.rs`) creates a Session Core managed run, classifies, completes via GatewayDirect, returns `run_id`/`thread_id`; Verevon gateway forwards prompt only; SPA surfaces run id (`a4578c505`).
- **Pending:** cross-plane audit event itself is Session-Core-owned via the managed-run lifecycle (D4). Local-stop stays honestly labelled until D4 lands.
- **Evidence:** SC3.

### Phase 4 — Permission hub parity with Codex (slice 3 SPA done, D1 pending)
- Slice 3: capability overview in `BrowserSitesSection` (browsing user-settable; downloads ask-every-time; uploads not allowed; recording never; profile ask) (`6adba79ab`).
- **Pending (D1):** org-core migration for new levels + broker enforcement + Quarry gate. SPA overview alone is not enforcement.
- **Evidence:** SC7 (hub renders; backend refusal tested once D1 lands).

### Phase 5 — Content & utilities parity (slice 5 local + slice 6 gateway done)
- Slice 5 local: bookmarks module `verevon.browser-bookmarks.v1`, star + bar in `BrowserChrome`, ZDR purge via `home-local-content` `clearBookmarks` (`6d7e08478`, `VITE_BROWSER_BOOKMARKS`).
- Slice 6 gateway: real PDFs served as `application/pdf` only when bytes start `%PDF-`, else image fallback (`dfe8bb618`).
- Slice 6 SPA: zoom 50–150% (`VITE_BROWSER_ZOOM`, `a41cb4273`).
- **Pending:** find, area screenshot, device-metrics/viewport emulation need Quarry actions; server history log needs D6 retention design.
- **Evidence:** SC4 (bookmarks purge), SC5 (PDF content-type).

### Phase 6 — Planner & agent depth parity (slices 8/9/10 SPA cores done)
- Slice 8 SPA: existing `DomNodeList` + interactive-only filter `VITE_BROWSER_ELEMENT_FILTER` (`a41cb4273`). Planner-side tuning pending D3/R-27.
- Slice 9 SPA defence-in-depth: `sensitive-redact.ts` (`maybeRedact`) wired into `CoreShell.submitAssistant`, `VITE_ASSISTANT_REDACT` (`b0bb61f82`). `fill_secret` pending D2/ADR-2.
- Slice 10 SPA core: `replay-run.ts` `buildReplay` substitutes `{{vars}}`, fails closed on mismatch/missing var/unsafe URL (`1e129b953`). Step-picker UI pending D7/WS10 trace wiring.
- **Evidence:** SC3 + SC7.

### Phase 7 — Stack evidence SC1–SC7 (not yet run — see §6)
Every phase above is flag-gated and unproven until SC1–SC7 are recorded. This phase is the only path to merge.

### Phase 8 — Merge gate (approval-required)
- Merge `feat/browser-assistant-slices` **only** from a clean checkout, only after SC1–SC7 pass and D1–D8 are ratified or explicitly waived with the waived scope staying flagged-off.
- Pre-existing failures on clean HEAD must be recorded, not silently waived: gateway `domains::browser` 3 tests, `BrowserChrome.reload.test.tsx` stale heading test (see PROGRESS.md).

### Phase 9 — Runtime rollout (one flag at a time, after evidence)
- Flip flags **one at a time**: `MODEL_GATEWAY_ASSISTANT_NAVIGATION=true` first, then each `VITE_*` flag, re-running the affected SC gate after each flip. Never bulk-flip.
- Rollback per flag: set the flag false, redeploy the affected plane, confirm the old behaviour.

---

## 5. Feature parity matrix — Codex / Aside / T3-Chat vs Verevon governed browser

| Reference feature | Verevon status | Slice / flag |
|---|---|---|
| Live agent browser view with stop, replay poll, take-over | **Shipped behind flags** — `AssistantLiveBrowser`; WS frame transport pending | Phase 2 |
| Floating composer / dock positions / split view | **Shipped behind flags** | Phase 2 |
| Tab context menu, new-tab page, keyboard commands | **Shipped behind flags** | Phase 2 |
| Annotation mode (click/drag, comment, Adjust) | **SPA draft shipped** (click point + comment, clipboard); planner attachment pending D5 | Slice 4 |
| Per-site permission levels (browse/download/upload/record) + settings hub | **SPA hub shipped**; broker/Quarry enforcement pending D1 | Slice 3 |
| History search, per-entry remove, clear-data | **Local bookmarks shipped**; server log pending D6 | Slice 5 |
| Page utilities: find, zoom, print-to-PDF, area screenshot, viewport | **PDF gateway + zoom shipped**; find/screenshot/viewport need Quarry actions | Slice 6 |
| Numbered `element_index`, element table, closed op set, selective observation | **SPA filter shipped**; planner view pending D3 | Slice 8 |
| Sensitive placeholders + `fill_secret` | **SPA redaction shipped**; custody + `fill_secret` pending D2 | Slice 9 |
| Replay saved runs with variables, fail closed | **Core shipped** (`buildReplay`); step-picker UI pending D7 | Slice 10 |
| Confirm before sensitive actions | **Already in `browser_risk` gates** (WS11) | — |
| Developer mode / full CDP, desktop control, stealth, profile import, MCP exposure | **Rejected** (ADR-5/ADR-6/ADR-7) | — |

---

## 6. Stack evidence SC1–SC7 (all pending except as noted in PROGRESS.md)

- **SC1 — Compose smoke:** Model Plane compose up, model-gateway `/healthz`, gateway `/healthz`, SPA serves.
- **SC2 — ZDR refusal live:** create `assistant_task` session under ZDR org → 409 before grant; non-ZDR org unaffected.
- **SC3 — Navigation run persistence:** POST `/v1/ai/assistant-navigation` with flag off → refused; with flag on → `run_id`/`thread_id` present, run visible in session-core, audit subject per D4.
- **SC4 — Bookmarks ZDR purge:** bookmark set → `clearHomeLocalContent` → bookmark key absent.
- **SC5 — PDF content-type:** gateway artifact with `%PDF-` bytes → `application/pdf`; without → image fallback.
- **SC6 — Live view + shell:** annotation dock, tab menu, split view, floating composer, new-tab page each render only with their flag on.
- **SC7 — Flag-off parity + pre-existing failures:** all new UI hidden with flags off; known failures on clean HEAD reproduced and triaged (gateway `domains::browser` 3 tests, `BrowserChrome.reload.test.tsx`).

---

## 7. Flags — every flag defaults off

`ASSISTANT_NAVIGATION_ENABLED=false`, `MODEL_GATEWAY_ASSISTANT_NAVIGATION=false`, `VITE_ASSISTANT_DOCK_POSITION`, `VITE_BROWSER_TAB_MENU`, `VITE_BROWSER_BOOKMARKS`, `VITE_BROWSER_ANNOTATE`, `VITE_BROWSER_ZOOM`, `VITE_BROWSER_KEYMAP`, `VITE_BROWSER_NEWTAB`, `VITE_WORKSPACE_SPLIT`, `VITE_ASSISTANT_FLOATING`, `VITE_ASSISTANT_REDACT`, `VITE_BROWSER_ELEMENT_FILTER` — all unset/false. No live flag or container was changed. (`VITE_AGENT_BROWSER_WORKSPACE` / `VITE_AGENT_BROWSER_LIVE_WS` were orphan env declarations with no implementation; removed this turn — re-add only when the session-reuse / WS code lands, per D8.)

## 8. Worktree & branches

- Worktree: `C:\dev\CoresSystem-assistant-slices`, branch `feat/browser-assistant-slices` (local only, no push, no PR).
- Main checkout `C:\dev\CoresSystem` on `main` was not edited except `.plan-work/browser-assistant-review/` (REPORT.md, PROGRESS.md, this plan).
- Merge gate: `git merge feat/browser-assistant-slices` from a clean checkout only, when approved; runtime rollout flips flags one at a time after SC1–SC7 evidence.

## 9. Risks carried forward (from REPORT.md §8)

1. Default-ask treated as consent widens submitted-task access — surface in hub before GA (D1).
2. Two traces until D4 lands — navigation runs are durable but the cross-plane audit publisher is session-core-owned.
3. Observation redaction (WS12 S0–S7) is built but unwired at review time.
4. Take-over enforcement flag `QUARRY_EDGE__TAKEOVER_ENFORCEMENT` value unverified at review time.
5. Actionability (WS3.K) ships off by default — attached runs without trusted input risk silent synthetic-click failures (D3).
