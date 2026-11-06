# Verevon assistant / browser review — commit 6b538f324

**Headline verdict:** **Conditional pass, do not GA yet.** The commit does not break "Model
proposes, Quarry executes or rejects" and the new browser path is genuinely governed, but
it ships two assistant paths with no shared trace, budget or ZDR contract, and the new
`assistant_task` grant has a ZDR path that is issuable but unusable. Keep every new flag off
in production until the slices in §6 land.

- **A. Audit architecture:** Pass, with two reservations (ZDR attach refusal and WS6 budget
  source). Model never gets selectors/JS, site policy is server-side in three layers.
- **B. Two paths:** Fail as designed — see divergence table (§3). The router is shared, the
  governance is not.
- **C. Grant design:** Sound and tight; needs a ZDR decision and a rename of
  `assistant_interactive`. Leaked-token blast radius is one host, ≤5 min, no uploads/downloads.
- **D. References:** 7 adoptable ideas mapped to slices; desktop control, model-written JS
  and stealth explicitly rejected. MCP stays parked (ADR-7).
- **E. Missing:** history/bookmarks, page utilities, tab strip, floating composer and the
  permission hub are all still missing. The dock and site-settings pieces are partial.

**What I verified by running:** `git show` / `git status` / `git worktree list` /
`git merge-base --is-ancestor` (commit `6b538f324` is an ancestor of `main` HEAD
`3bb5c96db`; working tree clean), four `webfetch` calls to the reference URLs and two raw
README fetches, plus `read`/`grep` over the audit, plan, contracts and commit. **What I
only read:** everything else — no tests, no builds, no containers, no stack, no flags were
touched. No code was changed. No confirmed ZDR or security leak was found that requires a
fix behind no flag, so no code fix was made; the two candidate leaks are listed as
unverified in §3 and must not be "fixed" without a reproduction.

**Paths in this report:** every `file:line` is at commit `6b538f324` unless stated.

## 1. What was read

In order: `C:\dev\verevon-audit-2026-10-14\02-quarry-and-browser-audit.md` (F-t2-001,
010, 011, 012, 013, 015, 032, 036, 063, 068, 069, 070) and its evidence index
(`audit-evidence\t2\` — I listed it but did not re-read every evidence file),
`03-model-plane-chat-and-agents-audit.md` (agent-loop grep only), `docs/plans/quarry-browser-build-plan-2026-10-14.md`
(build ledger, §2.1 decisions, WS3.K, WS7, WS8, WS9, WS11, WS12, WS13), `docs/BROWSER_WORKSPACE_PLAN.md`,
`apps/Ingestion Plane/Quarry-v2/docs/CONTRACTS.md` and `RUNBOOKS.md`,
`BROWSER_OMNIBOX.md` and `BROWSER_APPROVAL_CARD_E3_2B.md`, then the commit below, then the
four references. The commit's distribution-plan docs were ignored per instructions.

## 2. A. Does the commit follow the audit architecture?

| Check | Verdict | Evidence |
|---|---|---|
| **Model never gets selectors / JS** | **Pass** | The SPA never builds a selector or script. The browser agent sends `goal` + `target` only and renders `event.selector` from an approval event as a display string (`assistant-browser-agent.ts`, approval section). The navigation agent accepts exactly one typed JSON proposal (`assistant-navigation-agent.ts`, `assistantTaskProposalSchema`) and the gateway restricts `href` to a closed 8-value list and `query` to ≤160 chars with "Omit … secrets, and URLs" (`apps/gateway/src/domains/ai.rs`, `ASSISTANT_NAVIGATION_INSTRUCTIONS`). The gateway hard-denies `"evaluate"` with `browser_action_denied` (`apps/gateway/src/domains/browser.rs:5091`). This preserves ADR-6 (ratified 2026-10-28: closed declarative lists, `Evaluate` stays denied) and F-t2-002. |
| **Every browser action through Quarry with run grants** | **Pass for the browser path; reservation for the navigation path** | `create_session` issues a broker grant *before* Quarry is asked for a lease (`browser.rs:672-746`, `issue_session_grant`); `start_ai_run` forwards the session grant and model-gateway derives the run grant (`browser.rs:2215-2230`). The broker validates `assistant_task` (below) and Quarry re-runs its admission chain with the attachment gate added (`agent_policy_gate.rs`, `attachment_policy` / `gate_attachment_action`). The navigation path does **no** Quarry call at all — it is one model call + a public search, then hands off to the browser agent. It issues no grant, consumes no budget and leaves no run record (see §3). |
| **ZDR propagates** | **Reservation** | `create_session` carries `zdr` from server-side posture, never the body (`browser.rs:697`); `start_ai_run` re-derives `effective_zdr` from current org posture (`browser.rs:2100-2110`); the navigation gateway fails closed (`ai.rs`, `apply_org_zdr_posture`, tested for known-ZDR and unknown orgs). **But:** attach is refused whenever `effective_zdr` is true (`browser.rs:2125-2132`, `browser_attach_zdr_unsupported`) and `capabilities` returns `unavailable` for ZDR orgs (`browser.rs:640-645`). A ZDR `assistant_task` grant is therefore issuable but unusable for its own purpose (§4). |
| **Budgets (WS6) apply** | **Reservation** | The SPA caps at 20 steps / 120 s (`assistant-browser-agent.ts`) and the gateway forwards `max_steps`, `max_runtime_s`, `max_cost_usd` (`browser.rs:1885-1895`). Quarry's attachment budget lane (R-8) is used. **But** the broker *rejects* any budget claim on an `assistant_task` grant (`assistant_task.go`, budget-emptiness check), so the WS6.E grant-carried ceiling never applies to this path — caps come from the start body only, with no `max_cost` set by the SPA. |
| **Take-over (WS2 / WS8) applies** | **Pass, with a flag caveat** | `AssistantRetainedBrowser.tsx` claims the retained session via `changeBrowserControl(..., 'human_takeover', epoch)` and only then enables human actions; the `USER_TOOK_OVER` contract is unchanged (`CONTRACTS.md` §9, `QUARRY_EDGE__TAKEOVER_ENFORCEMENT` — I did not verify the flag's value in the shipped compose, so enforcement may still be observe-only). |
| **Site policy server-side, not SPA-only (WS7)** | **Pass** | Three server layers, no SPA authority: org-core answers `explain` (owner of policy, per ADR-1 outcome C); the broker *always* consults it for `assistant_task`, even when ordinary grants do not (`assistant_task.go`, `checkAssistantTaskSitePolicy`); the gateway walks the redirect chain and checks each hop's access *before* probing Quarry (`browser_redirect.rs`, `resolve_chain`); Quarry enforces the attachment as an independent layer that cannot be weakened by observe mode (`agent_policy_gate.rs`, `attachment_policy`). The SPA only renders the outcome and the "Allow for 10 minutes" choice (`GlobalAssistant.tsx`, site-access section). |

**Net:** the commit strengthens the governed core (WS1/WS7/WS8) rather than bypassing it.
Its weaknesses are scope gaps (a second ungoverned path, a ZDR dead-end), not architecture
violations.

## 3. B. Two assistant paths next to the Model Plane run

The navigation agent (`assistant-navigation-agent.ts` + `ai.rs`) and the browser agent
(`assistant-browser-agent.ts`) are two assistants beside the durable Model Plane run. One
router (`assistant-task-routing.ts`, `submitAssistantTask`) is shared and `isAssistantTaskBusy`
correctly blocks a second task while either is starting/running/stopping/disconnected. The
governance underneath is not shared.

| Dimension | Navigation agent | Browser agent | Divergence |
|---|---|---|---|
| What it does | One `POST /api/v1/ai/assistant-navigation` call (`ai.rs`, temp 0, ≤200 tokens) returning `guide` / `browser.current` / `browser.search` / `unsupported`; for search it runs `runWebSearch({publicOnly:true, limit:8})` and filters to providers `searxng/stract/brave/serper` (`assistant-navigation-agent.ts`) | Capability check → `createBrowserSession(purpose:'assistant_task')` → redirect resolution → `startBrowserAiRun({requireAttached, assistantInteractive, maxSteps:20, maxRuntimeSeconds:120})` → SSE + 3 s replay poll → retained session → take-over | Different backends, different failure modes, different evidence. |
| Site policy | None before search; the *target* is only checked later, downstream, when the browser agent creates its session | org-core per-host check at issue + per-hop redirect check + Quarry gate | Search can surface a denied site's title/URL before any policy check. **Unverified** whether `runWebSearch` is ZDR-gated server-side — the SPA passes no ZDR signal. |
| Budget | One inference call; 4 096-byte prompt, 8 KiB body (`ai.rs`). No step/time/cost ledger | 20 steps / 120 s; Quarry budget lane; no `max_cost` set | No shared ledger; nav spend is invisible to WS6. |
| Trace | Local status only (`idle/thinking/stopped/error`); no `runId`, no replay, no audit event | `BrowserAttachSharedJarEvent` receipt, approval events, run events, Quarry timeline, retained session | Violates F-t2-014's "one canonical trace" if nav is counted as an agent run. |
| ZDR | Inference call carries `zdr:true` on posture-lookup failure (fail-closed, good) | Attach refused under ZDR; capabilities unavailable | Opposite defaults: nav *runs* ZDR, browser *refuses* ZDR. |
| Approvals | None (no clicks possible) | Risk-category approvals with `canApprove` allowlist; decision recorded, action not resumed | Correct asymmetry, but nav has no way to ask for anything. |
| Stop | Local `AbortController`; code comment admits "The model endpoint does not expose a provider-side cancellation receipt" | `control(..., 'stop')` with confirmation; `disconnected` state if unconfirmed | "Stopped" means different things. |
| Selectors/JS | None (closed hrefs, search query) | None (goal only) | **No divergence — both pass.** |
| Consent | None | 10-minute `ask → act` choice (`allowSiteAccess`, origin-checked against `explanation.revision`) | Nav cannot be consented to. |

**Verdict:** the router shares "is something running?", nothing else shares a contract.
The cheapest fix is to give the navigation path a run record and an audit event (slice 2),
not to merge the two paths into one backend.

## 4. C. Design note — `AssistantTask` purpose and `assistant_interactive`

**What they are.** `BrowserSessionPurpose::AssistantTask` (`browser.rs:135-138`) marks a
`create_session` call as a submitted assistant task. `StartAiRunBody.assistant_interactive`
(`browser.rs:412`) is a boolean on `POST .../ai-runs` requesting the bounded interactive
assistant policy on an attached run. Neither grants power by itself: the first routes to a
tight grant kind, the second *removes* capabilities.

### `assistant_task` grant (broker `assistant_task.go` + `grant.go`)

- **Granted:** one cloud browser session, exactly **one host**, for a **signed-in user**
  principal only (`PrincipalType == user`, non-empty `ActorID`), behind the opt-in
  `AssistantTaskGrantsEnabled` broker option and the gateway's
  `BROWSER_ASSISTANT_TASK_GRANTS` flag (compose default `false` — ships dark).
- **Scope:** `scopeURL` is rewritten to `browser://cloud/assistant-task` and stored on the
  grant; run-grant derivation then requires **exact host equality** (`hostWithinGrant` in
  `grant.go`), so a subdomain is refused (tested: `docs.example.com` child "widened to a
  subdomain"). No parent grant, no `AllowedActions`/`FrameIds`/`DialogIds`/`ArtifactIds`,
  not read-only, and any budget-claim field is rejected. The grant therefore authorises
  ordinary act-class steps only — dialogs, uploads and downloads have no sensitive-approval
  scope to draw on.
- **TTL:** capped at **5 minutes** (`assistantTaskGrantTTL`, `min(ttl, 5m)`), and expiry
  equals absolute expiry — a fixed, non-renewable window. Derived run grants must fit
  inside it.
- **Site ceiling:** allowed iff org-core says `Act`, or (`Ask` + cap `act` + `default`
  source). That third clause is the novel risk: a *default* `ask` is treated as consent
  for a submitted task. Explicit member/org decisions always win (tested, including
  read-ceiling and org-deny cases). The gateway's redirect walker applies the same rule
  per hop.
- **ZDR:** the gateway forwards `zdr` into the grant body (`browser.rs:5345`), and a ZDR
  grant is memory-only per `RUNBOOKS.md` (ZDR grants are never persisted). But the same
  gateway then refuses to attach any ZDR-effective run, and `capabilities` reports
  unavailable for ZDR orgs. **The issuable-but-unusable ZDR path is the one defect in an
  otherwise tight design** — either refuse ZDR at `create_session` for this purpose, or
  document the assistant as a non-ZDR feature. Owner decision needed (slice 1).
- **Revocation:** the generic broker store (Postgres for non-ZDR, cascade revoke with an
  advisory lock, WS1.C) plus Quarry session close / run close. There is **no per-task
  revoke button** in the SPA, and policy tightening does not push revocation (WS7.M is
  optional and off). A grant lives until expiry or an explicit broker revoke.

### `assistant_interactive` flag

Not a grant and not a permission: the gateway validates it (`require_attached` mandatory,
`plan_mode` forbidden, domains mandatory unless continuing in a tab), then pins
`policy.disable = ["browser.act.upload", "browser.act.download"]` (`browser.rs:1904-1906`).
Model-gateway dispatches it as a *restricted* step and Quarry resolves it as an enforced,
tighten-only attachment layer (`agent_policy_gate.rs`). The name is misleading — it means
"the assistant may interact with the attached tab", not WS8's human-interactive class — and
should be renamed or documented. All its flags (`BROWSER_AI_RUN_ATTACH_SESSION`,
`MODEL_GATEWAY_BROWSER_ATTACH*`, broker `AssistantTaskGrantsEnabled`) default off.

### If a token leaks

A leaked **grant id alone** authorises nothing: Quarry validates through the broker against
the grant's owner/org. A leaked **user bearer** (within its own lifetime) could: issue an
`assistant_task` grant for one host where org policy is default-`ask`, open an ephemeral
Quarry session (no profile, no persistent cookies), and drive up to 20 steps / 120 s of
click/type/select/scroll/navigation on that exact host, watching the owner-only frame
stream for that grant. It **cannot**: reach a subdomain or any other host, upload or
download files, approve its own dialogs, use a saved profile or login, set a budget, attach
to another user's session, widen a Space grant, or persist anything under ZDR. Blast radius:
**1 host × ≤5 min × agent steps, no file transfer, no credential access.** That is the
right shape; the remaining fix is the ZDR dead-end and a revoke control.

## 5. D. What to adopt from each reference

**Reject rule applied to all four:** anything requiring desktop control (OS-level CUA),
model-written JS (ADR-6), or stealth/anti-bot (ADR-5) is rejected. MCP exposure is ADR-7
and stays parked.

### open-computer-use (desktop CUA as MCP)

Tools: `list_apps`, `get_app_state`, `click`, `perform_secondary_action`, `scroll`, `drag`,
`type_text`, `press_key`, `set_value` (+ macOS `select_text`, `query`).

| Adopt | Map to |
|---|---|
| **Numbered `element_index`** from the latest state, reused within one process; ambiguous targets fail closed instead of guessing the first match | **WS11** (planner view) — already have backend-node-bound refs and stale-target fails (`WS3.K`); add the numbered table lane for the planner only. S–M. |
| **`get_app_state` budgets** (`text_limit`, `max_tree_nodes/depth`) and **targeted `query`** instead of a full tree | **WS3.K / F-t2-034** — `query` by text/role without a snapshot is a new planner-visible lookup; never as model JS. M. |
| **Action read-back** (`set-value-verification`, `linux-action-read-back`): verify the value landed | **WS3.K** follow-up (trusted-input postconditions). S. |
| **Selective observation** (`tree-budget`, `selective-observation`) | **WS12 / F-t2-034** — cap projection by viewport/interactivity. M. |
| `doctor` permission onboarding | **WS9** settings hint only (no desktop perms needed). S. |

**Reject:** all desktop control (`click` at coordinates, `drag`, `perform_secondary_action`,
global pointer fallbacks), **Locked Use** (temporarily unlocking the user's Mac — directly
contradicts containment), local MCP exposure to the model (ADR-7).

### Codex in-app browser (docs)

| Adopt | Map to |
|---|---|
| **Annotation mode** — click/drag an element or area, comment, **Adjust** (font/text/spacing/colour) and send | **New slice §6-4 (WS9)** — annotations are governed attachments to the planner, not raw DOM; ZDR: comments are user-authored, page region stays ephemeral. M. |
| **Per-site permission levels** — defaults for browsing/downloads/uploads + per-site exceptions, org-restrictable | **WS7 / F-t2-032** — extend the existing `browser_sites` (deny/ask/read/act) with downloads/uploads/recording levels and a settings hub. M. |
| **History search, per-entry remove, Clear browsing data** by range and type | **F-t2-068 / §6-5** — server-derived visit log over WS8.B metadata receipts; ZDR orgs keep no entries. M. |
| **Confirm before sensitive actions** (submit, purchase, permission change, delete) | Already in `browser_risk` gates (WS11); keep, and surface the Codex wording next to the existing approval card. S. |
| Split view / floating composer over the page | **WS9.G/H / §6-7** (see §7 below). |

**Reject:** Developer mode / **full CDP access** (model-written JS by another name — ADR-6),
browser-extension attach to the user's real Chrome profile (breaks containment and ZDR
evidence), automated file uploads, profile import from the OS browser.

### jev-ultrafast (indexed action space, one decision per cycle)

| Adopt | Map to |
|---|---|
| **Element table `[n] role label · value`** with a closed op set (`CLICK/TYPE_TEXT/SELECT/SCROLL/WAIT/DONE/BLOCKED`) and only supported ops offered | **WS11** — fold into the planner view next to `element_index`; ops map 1:1 onto the closed `AgentAction` list (ADR-6 compliant). S–M. |
| **One request per decision** (operation+target in one round trip) and **no screenshots by default** (structured state only) | **WS11 / WS12** — aligns with Quarry's AX-first contract and the image budget (F-t2-034). S. |
| **Validate target before input** (freshness, occlusion) and **wait for useful state** (combobox 200 ms, else 2 frames) | Already **WS3.K** (hit-test, stability, trusted input); copy the *wait constants* as tuning data, not a new gate. S. |
| **Inspector "Choose next"** (pause before execution) | **WS2 / WS9** control strip — a UI pause, not a model feature. S. |
| `DONE` requires independent verification | **WS11** stop rules (already half-present via `check_limits`). S. |

**Reject:** TypeSafe speculative fan-out and the Mercury text model (different provider),
Browser Harness local Chrome, screenshot-label rendering as a control surface.

### browser-use (full agent loop + replay)

| Adopt | Map to |
|---|---|
| **Sensitive-data placeholders** — typed values saved as `<secret>key</secret>`, replay types the placeholder | **F-t2-024 / WS12** (`fill_secret` carried by Quarry, model sees only the placeholder, receipt carries a boolean). This is the correct shape for credential handoff; defer passwords until the custody ADR. L. |
| **Replay of saved runs with variables** — recorded clicks/typing replayed with new values, fail closed at the first mismatch | **WS10 / WS8** — Quarry already compiles procedures and compares replays; add variable substitution + per-step approval + zero model calls per step for the replayed prefix. M–L. |
| **Custom `Tools`** | Parked (ADR-7 / F-t2-063 code-mode). Do not adopt now. |
| Profile sync (cookies only), cloud stealth browsers, CAPTCHA solving, 31 direct MCP actions, Bash | **Reject** — stealth violates ADR-5; profile sync breaks containment; direct MCP actions bypass the grant/budget/trace chain; Bash has no place in the browser path. |

## 6. Ranked slice list

| Rank | Slice | Size | Plan slice |
|---|---|---|---|
| 1 | **Close the ZDR dead-end:** refuse ZDR at `create_session` for `assistant_task`, or explicitly scope the assistant as non-ZDR; add a contract test | S (≈2 d) | WS8.C / WS4 |
| 2 | **Give the navigation agent a run record and an audit event** (one `AuthzDecision`-shaped record per task; local-stop honestly labelled) | S (≈3 d) | WS10 / F-t2-036 |
| 3 | **Per-site permission levels hub** (browse/download/upload/record vs today's ask/act/read) | M (≈6 d) | WS7 / F-t2-032 |
| 4 | **Annotation mode** (element/area comment, Adjust, governed attachment) | M (≈5 d) | New, WS9 |
| 5 | **Server-side history, bookmarks, clear-data** over WS8.B metadata receipts | M (≈6 d) | F-t2-068 / WS8.B |
| 6 | **Page utilities** (find, zoom, print-to-PDF, area screenshot, viewport) | M (≈4 d) | F-t2-069 / WS13 (PDF/screenshot already built; gateway allowlist + UI do the rest) |
| 7 | **Tab strip polish + dock layouts + floating composer** | M–L (≈7 d) | F-t2-070 / WS9.G–H |
| 8 | **Element table + selective observation** (jev + OCU query/verify) | M (≈5 d) | WS11 + WS3.K |
| 9 | **Sensitive placeholders + `fill_secret`** | L (≈10 d) | F-t2-024 / WS12 (after custody ADR) |
| 10 | **Replay saved runs with variables** | L (≈8 d) | WS10 / WS8 |

## 7. E. What is still missing after this commit

- **History/bookmarks (F-t2-068):** still the browser-local 10-entry list plus the Quarry
  owner timeline. No bookmarks, no agent/user source filter, no clear-data dialog, no
  per-page permission prompts. WS8.B's metadata-only human receipts are the store; the UI
  and cross-run log are not built.
- **Page utilities (F-t2-069):** find, per-tab zoom, print-to-PDF (the gateway coerces media
  type and rejects pdf), area screenshot, device-metrics/viewport emulation, colour scheme,
  mute/DevTools extras. Driver capture for full-page/PDF exists; the gateway/SPA path is the
  S-sized remainder.
- **Tab strip (F-t2-070):** no tab context menu, no new-tab page, no split view, no central
  keymap, no activity strip, no reader view. Typed error cards are partial
  (`BrowserBlockedNotice`, strong-challenge only).
- **Floating composer / dock (F-t2-012):** `GlobalAssistant` is a non-modal dock, but there
  are no dock positions (left/right/bottom/pinned/window), no always-available floating
  composer over the page, and the assistant's browser runs are **not bound to the chat run**
  (no thread lifecycle) — the chat's own live-run panel is still the read-only
  `BrowserChrome` mount.
- **Permission hub (F-t2-032):** `browser_sites` gives per-host deny/ask/read/act, but there
  is no Settings > Browser hub showing defaults and exceptions for browsing, downloads,
  uploads, recording, profile use and Developer access, no turn/conversation/always consent
  scopes, and no 16-reason typed-denial catalogue surfaced to the user.
- **Also still missing (not asked, for the record):** free human browsing (ordinary sessions
  are still first-host / 12 steps / 120 s; `human_browse` is flag-off), planner default-on
  (WS11 stage 3), code-mode (F-t2-063, parked per ADR-6), MCP exposure (ADR-7, parked).

## 8. Risks

1. **Default-ask treated as consent** (`assistant_task.go`) widens what a submitted task can
   open without an explicit user choice. Mitigated by org/member decisions winning, but the
   default posture should be surfaced in the hub before GA.
2. **Two traces / no nav trace** means support cannot follow a navigation task across planes
   (F-t2-014, F-t2-036).
3. **Observation redaction is not yet enforced** (WS12 S0–S7 built but unwired at the time
   of the plan) — assistant observations reach the planner unscrubbed; this commit does not
   change that.
4. **Take-over enforcement flag** — confirm `QUARRY_EDGE__TAKEOVER_ENFORCEMENT` is on before
   relying on §2's take-over row.
5. **Actionability (WS3.K) ships off by default** — attached runs without trusted input are
   vulnerable to silent synthetic-click failures.

## 9. Owner decisions needed

1. **ZDR assistant availability:** refuse ZDR at issue, or ship a ZDR attach lane? (slice 1)
2. **Default-ask consent:** keep "submitted task = consent for default sites", or require an
   explicit choice per site?
3. **`assistant_task` TTL:** 5 min fixed, or align with the 15-min session grant (WS8)?
4. **Budget source:** keep start-body caps, or mint a WS6.E grant claim so the org ceiling
   applies per run?
5. **Nav trace retention:** how long are navigation run records kept, and what is stored
   under ZDR (metadata only)?
6. **Annotation + history retention** under ZDR (comments are user-authored; page context is not).
7. **Rename `assistant_interactive`** (it is not the WS8 interactive class) — or document it
   in place + docs for the four new flags.
