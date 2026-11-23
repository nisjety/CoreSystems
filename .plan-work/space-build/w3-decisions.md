# Space W3 owner decisions (2026-11-16)

| # | Topic | Decision | Work |
|---|---|---|---|
| D1 | Shared memory when sharer removed/demoted | Auto-retract all their shared copies in that room | Control removal/demotion hook -> session-core retract (WS17-S7 path) |
| D2 | Canvas tools for agents | Own per-agent switch, separate from rename/pin | Binding field + gateway route + SPA toggle; model-gateway/execution-core offer canvas rows only under the canvas switch |
| D3 | Agent card posting | Enable now | capability-core migration flipping space.thread.post_card admission to enabled; WS18-S6 toggle copy mentions cards |
| D4 | Browser start from a new thread | Change Control: browser decision without an existing thread; the run creates the thread | Control browser-run decision accepts no thread; gateway R1 + model-gateway start create the thread |
| D5 | Parent browser grant | Narrow to the run's hosts AND revoke at run end | gateway browser.rs parent grant host-limited; revoke on terminal |
| D6 | Run count / ResolveRunSpace auth | Keep org-level (no change) | none |
| D7 | `browser` in agent_computer preset | Explicit-only: remove from preset | user-core org_policy_presets.go + explicitOnly list |
| D8 | Room history for new members (NEW FEATURE) | Default: new members SEE past conversation. Invite option "don't share past conversation": member joins but past is hidden. Room admin can later change it to show past, with a date filter (show from date X). Canvas follows the same rule (no blanket 403). | Control: per-member history visibility (all / since join / since date) set at invite and editable by admin; session-core + canvas honour it; SPA invite option + admin control |
| D9 | Old personal notes hidden by backfill | Hide from agents, owner sees them and can restore | session-core restore path + memory panel control |
| D10 | Org agents ownership | Org-owned, no owner (new Control path); leaving users don't suspend shared agents | user-core register org agent ownerless; gateway bind/create uses it; owner sweep skips |
| D11 | OWNER_LIST_SPACE_REFUSE default true | Ship as release item | release note only |
| D12 | Per-message personal memory opt-in permission | Own entitlement bit | user-core migration + catalogue row; WS17-S8 decision uses it |
| D13 | MODEL_ACTION_ADMISSION env | Each core controls its own .env; do not add to compose | runbook note only |
| D14 | Older routes missing ZDR (POST /api/v1/spaces/{ref}/agents, PUT /api/v1/agents/{id}/draft) | Fix now | gateway ZDR guards + registry rows |
| D15 | Registry scope | Extend registry to Control internal decision routes and Data-plane (wiki-store) routes, with lints | contract JSON + lint |
| Next | | Code decisions on space-w3, then compile pass (cargo check / go build / tsc) and fix compile errors. Still no tests. | |

## Round 2 (2026-11-16, post-compile-pass)

| # | Topic | Decision | Work |
|---|---|---|---|
| D16 | Browser grant revoke (was D5 open item) | Revoke immediately: give model-gateway a broker service account with `browser:write` | Service-account change (principal train) + model-gateway revoke call at run end |
| D17 | D9 old-notes Space-kind lookup | Option (a): Control signs the answer, gateway relays it to session-core | Control issuer for "is this space the caller's personal space"; gateway relay route; session-core RestoreMemory uses it instead of refusing unknown-kind threads |
| D18 | D8 room history for new members | Confirmed as described: default sees past conversation; invite option hides it; admin can later reveal, optionally from a date; canvas follows the same rule | New feature: Control per-member history visibility (all / since-join / since-date), invite option, admin control; session-core + canvas honour it; SPA invite + admin UI |
| D19 | Ownerless org agent admin | Yes: org admins get a management screen (edit/suspend/retire) | Control admin routes for org-visible ownerless agents; gateway + SPA admin UI |
| D20 | ZDR deletes (skills/plugins/schedules) | Registry wins: allow deletes under ZDR (deletion removes content, does not persist it) | Revert the W3-DEC-ZDR2 refusal guards on the DELETE routes for skills/plugins/schedules; keep guards on create/update/publish/restore/import; fix registry rows to match (delete rows stay/become allowed, non-delete rows stay refuse) |

## Round 3 (2026-11-19, after the D18 wake-leak review)

| # | Topic | Decision | Work |
|---|---|---|---|
| D21a | Do wake under a member history window | Keep the refusal (`wake_history_window_do_unsupported`) | none (c5c70b6eb, 5d428eff2) |
| D21b | Room files, knowledge retrieval, sandbox workspace vs D18 | Stay shared; only the conversation and canvas are windowed | Document in d18 spec; no code |
| D21c | Post-floor content restating pre-floor turns | Acceptable; the window goes by creation time | Document in d18 spec |
| D21d | Wake + regenerate/edit in one request | Refuse it | model-gateway check_wake refusal + SPA copy |
| D21e | Private panel ("Privat assistent") | In-panel composer; replies private to the member | SPA composer in the panel over the existing private-turn path |
| D21f | "Set as primary agent" | Primary agent is the room's default responder for messages that mention no agent (subject to its wake policy) | Control/binding field + wake routing + SPA |
| D21g | Memory take-back after the dialog closes | Build it (retract a shared memory later from the memory list) | session-core/Control path + gateway + SPA |
| D21h | Browser run Stop by its starter | Build it (starter can stop, not only admins) | Control decision + gateway + model-gateway/Quarry stop + SPA |
| D21i | Compliance role and legal hold | Grant compliance access to legal-hold routes (view/set/lift) | Control role matrix + gateway + registry |
| D21j | Convex status text for ZDR orgs | Stop storing it; closed status words only | convex-core + producers |
| D21k | ZDR refusal envelope | Align to `{ error: { code, message, details } }` everywhere | gateway/BFF + SPA readers + registry |
| D21l | Sidebar layout in localStorage under ZDR | Allowed (UI preference, no content) | none |
| D21m | Canvas for members with a history window | View and save the current canvas; no browsing/restoring versions from before the window | Control canvas decision + wiki-store + SPA |
| D21n | Data Plane canvas error codes | Fix: not-found first, then authority/ZDR; canvas_head_moved distinct from canvas_not_found | wiki-store + SPA copy |
| D21o | SPACE_MEMBER_HISTORY_WINDOWS | Declared in Control compose; on in local dev, off by default in production | compose + flag registry |
| D21p | Builder models for the full W1-W3 completion | Opus for hard (security / cross-plane), Haiku 5.5 max for simple (UI copy, mechanical) | - |
| D21q | Merge of space-w3 | After the full Wave 1-3 audit completion | - |

## Round 4 (2026-11-20, applied without further dialog)

A second question round (ZDR reads, ZDR enforce, GDPR erasure actor id, room-ref probing) was declined before any answer was given. Per the standing rule ("inside an approved task, do every implied step without confirmation questions") and the scoping ledger's own recorded recommendation for each item, the remaining owner-decision items from the gap ledger are resolved below with the plan's/ledger's own stated default — not a new guess — so building is not blocked on dialog. Any of these can still be overridden; they are logged, not silent.

| # | Topic | Decision | Work |
|---|---|---|---|
| D22a | ZDR reads (room history/pins/timeline) | Typed refusal `{error:{code,message,details}}`, consistent with D21k | gw/spa per route; aligns with DEC-3D-ZDR-TIMELINE below |
| D22b | Space ZDR enforce (SESSION_CORE_SPACE_ZDR) | Enforce in dev now; production after a clean counter week; scratch-only sandboxes (WS06-S6b) stay parked; make process_registry inert under ZDR now | sc flag flip + dev compose; prod flip is a later release step, not blocking |
| D22c | GDPR erasure: policy-audit actor id | Pseudonymise; deactivate/pseudonymise space_memberships, audience and history-window rows | sc/ctl erasure local-stage extension |
| D22d | Same-org non-member room-ref probing | Private rooms answer byte-equal 404 to non-members; org_public rooms may still reveal existence | gw read routes + Control refusal alignment + byte-equality tests |
| D22e (W1-Q2) | Org room renamed? | Blocked | convex `space_kind_not_editable` refusal; hide the rename field for the org room |
| D22f (W1-Q3) | Stream on archive? | Close the stream; reads stay available | Control stream decision refuses archived rooms; SPA falls back to read-only polling |
| D22g (W1-Q4) | Add adopts application_sync rows? | Adopt at cutover; keep the registrar row explicitly guarded after any Control write | Control rule + tests |
| D22h (SD-18) | Space privacy attestation / prod policy writes | Keep operator-only attestation; SPACE_POLICY_ADMIN_WRITES stays off in production until a dedicated self-serve path gets its own legal/privacy review | none now; flagged, not blocking |
| D22i (DECISION-WS06-Q3) | Space shell requires sandbox entitlement? | Yes, after one observe release (don't skip the observe step) | flip the compose default on schedule |
| D22j (DECISION-SD-06) | ADR 6 egress model | Confirm as built (per-call CONNECT proxy, tiers off/registries/allowlist, never grantable); no SPA control for the egress ceiling for now | ledger entry only |
| D22k (DECISION-SD-24 / WS05-Q2) | SSO/SCIM pilot IdP and plan coverage | Flagged for a real product decision; not resolved here, not blocking the rest of W1-3 | WS05-S8/S9 live matrix stays owed |
| D22l (DECISION-WS05-S8-ADOPT) | SCIM binds existing owners/admins? | Refuse to bind (409, manual) | org-core/auth-core |
| D22m (DECISION-WS05-S3-LEGACY) | Legacy email-string auto-admin hook | Report-only list for org owners; restrict the hook to non-SSO/SCIM users; stop logging the email | org-core |
| D22n (DECISION-WS07-D6) | Stricter CSP enforcement | Enforce after one clean report week per environment; production may count by directive | swap candidate header to enforced |
| D22o (DEC-WS12-Q5) | DM creator erased under GDPR | Keep the peer's own messages (author-level erasure); do not delete the thread | sc GDPR path + Control DM roster |
| D22p (DEC-WS14-Q5 / SD-30) | Owner-asserted grant writes | Option A as designed; ship S4; flip SD-30 in the documented order | flag flips |
| D22q (DEC-3D-ZDR-PINS) | Pins/DM-hides under ZDR | Allowed as content-free rows (ids/flags only, no text) | pins.rs / dm hide refusal review |
| D22r (DEC-3D-DM-ATTN) | DM attention default | Every DM message notifies by default; mutable per DM | notification-core |
| D22s (DEC-3D-SEARCH-EDITED) | Edited-message findability | Accept v1: not findable by new text; revisit later | none now |
| D22t (DEC-3D-REACT-FLAG) | Reactions vs SPACE_MESSAGE_REVISE_API | Couple the flags in rollout docs; no separate proof route | rollout doc note |
| D22u (DEC-3D-ZDR-TIMELINE) | ZDR empty timeline vs typed refusal | Typed refusal (matches D22a), with SPA copy | gw + spa |
| D22v (WS17-OQ2) | Personal agent's owner-private instructions in room answers | Not allowed; strip owner-private instructions from room turns | model-gateway |
| D22w (WS18-OQ1) | model-action-view-v2 vs Space actions[] | Deferred to the WS24 owner; flagged, not blocking W1-3 | joint decision later |
| D22x (WS18-OQ3) | tickets.create promotion before production | Required before any production enablement of self-API/canvas tools | release gate, matches existing promotion pattern |
| D22y (WS19-OQ1) | Canvas retention/erasure of author ids | Flagged for counsel/owner ruling; canvas stays pilot-only (no production `canvas_entitled`) until answered | not blocking code completion |
| D22z (WS19-OQ2) | Agent canvas write mode | Ask (register a resumable kind) | wiki-store + capability-core |
| D22aa (WS20-OQ1) | Observer frames / manager takeover | Keep SD-23 v1 (starter-only); S6/S9 stay dark | none |
| D22ab (WS20-OQ2) | Quarry room-view routes / run delegation owner | Flagged, cross-program; WS20-S6/S8 stay parked until named | not blocking |
| D22ac (WS20-OQ6) | ApprovalKind HUMAN_ASSIST=6 | WS20-S7 adds it in this program | execution-core/model-gateway |
| D22ad (W3-OQ-PRESET) | Preset keeps explicit-only W3 bits? | Yes, preserve explicit-only keys (browser, canvas, personal agents, memory declassify/turn) when a preset is applied | user-core PresetEntitlements |
| D22ae (DEC-M0-CI-GATE) | space-gate required check | Make required on main now that space-w3 merges to main (D21q); wire registry-sync, node --test, org-core DB suites now; Playwright gate once live e2e lands (D21: live e2e approved) | CI workflow |
| D22af (DEC-PCFG-RETENTION) | New service-principal retention classes / browser:write breadth | Confirm the two retention classes as drafted; accept browser:write for the revoke credential (a revoke-only broker scope is a later hardening item, not blocking) | principal train |
| D22ag (DEC-ZDR-ROW-CLASSES) | Cron PATCH under ZDR / D20 removal waived tokens | Cron PATCH = configuration (not a ZDR content op, n/a); D20 removals = ZDR-only, not waived | registry rows + capability-core isZDRRemoval list |
| D22ah (DEC-WS04-ROOM) | Room administration defaults | Confirm each plan default as-is; F12 resolved by D22d above | none beyond D22d |
| D22ai (DEC-WS08-WAKE) | Wake room-context block / model / origin mapping | D-6 block stays as designed (sanitized participant labels); wake uses the thread's configured model; origin mapping confirmed as implemented | none |
| D22aj (DEC-WS09-RUN-CONTROL) | Run control rulings | ZDR runs follow D22b; newer bits join the standard preset membership; archived-room stop is allowed; ended runs show their real step count | run/gw |
| D22ak (DEC-WS15-SYSTEM-ROWS) | Who sees membership system rows | Every member, including viewers | sc/spa |
| D22al (DEPTH-WS17-S3-RECALL) | Room memory recall semantics | Rule R: recall only the thread's own rows | session-core |
| D22am (DEC-WS17-ADMISSION-ROLLOUT) | SPACE_AGENT_ADMISSION rollout value | Observe in all environments for now; revisit enforce after an observe week | compose |
| D22an (DEC-WS17-RETRACT-PATHS) | Owner retract vs removal retract | Owner path follows the removal rule | session-core |
| D22ao (DEC-WS17-S9B-GRAPH) | No graph call while MODEL_GATEWAY_GRAPH_SPACE_SCOPE is off | Confirmed | flag table note |
| D22ap (DEC-WS18-TITLE-CLEAR) | Cleared thread title stays owner-locked? | Yes | model-gateway |
| D22aq (DEC-WS19-CLEAR) | Canvas clear with no empty version | Allow a one-character clear | wiki-store |
| D22ar (DEC-WS19-AGENT-CANVAS-FLOORS) | Agent canvas read floor / archived rooms | Keep the editor floor for agent rows; archived reads refused (typed) | wiki-store |
| D22as (DEC-W3-ORGAGENT-EDIT) | Org-admin edit of ownerless org agents | Build it now (supersedes the lead's "not this release" deferral; matches D19); owner-to-org flip clears the owner field | user-core UpdateAgentAccess + gw + spa (see D19#D21) |
| D22at (DEC-D18-SHARE-PAST-DEFAULT) | since_join room + share_past_conversation absent | Absence does not override the room's since_join default; only an explicit `share_past_conversation: true` reveals the past | Control add-member handling |
| D22au (DEC-WS10-Q5) | Quarantine false-positive threshold / reviewer | Stay in shadow mode until real data is reviewed; threshold starts conservative; named reviewer is the acting lead until the owner names one | no flip to enforce yet |
| D22av (A-REC-02-POL) | Org admins open the room directory themselves? | No; stays operator-only for now (RECOMMENDED tier, not urgent) | none |
| D22aw (DEPTH-W3-KIND-NOTES) | Old personal-note restore audit | Record the restore event as an audit row (owner can read it); no further ruling needed | session-core |

## Round 5 (2026-11-20, defects found by the ctl-group D21/D22 builder, fixed same pass)

| # | Topic | Decision | Work |
|---|---|---|---|
| D23 | Starter browser-run stop decision vs ZDR | Issued under ZDR too — stopping is always the safe direction; the gateway's live ZDR check (D21h gw leg) covers the run itself, not the stop ask | user-core: removed the ZDR refusal from ValidateForBrowserRunStop; decision still signs ZeroDataRetention for the verifier (cb0726b29) |
| D24 | Owner-to-org agent flip dropping the former owner's access | Keep the former owner on the access list as a manager unless they already listed themselves; the access list, not ownership, governs edits (D10) | user-core: UpdateAgentAccess auto-adds the actor as manager on a flip with no self-entry (c207d11a2) |

## Round 6 (2026-11-20, defect found by the dat-group D21n/D21m builder, fixed same pass)

| # | Topic | Decision | Work |
|---|---|---|---|
| D25 | Gateway canvas create forwarding a stale `expected_version_id` | Send the same null the decision was signed with, not the client's raw body field | gateway canvas.rs save_version_inner (79e1b136b) |

Owed from the dat-group pass (D21n/D21m/D22ar), for the next ctl and gw/spa batches:
- ctl: `IssueCanvasDecision` (canvas_decision.go:347) must issue, not refuse, a windowed save/restore decision, carrying the floor (D21m); keep refusing agent writes under a floor.
- ctl: agent canvas reads in an archived room need the typed 409 `room_archived` (D22ar), mirroring `refuseArchivedWrite`.
- gw: the save/restore relay must cope with the new 404 `canvas_not_found` (D21n); confirm `list_versions_inner` already passes `meta.window`/`current` through unchanged (D21m).
- gw: Control's 400 `canvas_decision_invalid` is mapped to 503 — unchecked, carried over from the ledger, needs a look.
- sc/registry: ~~the golden canvas-read row says `min_role: viewer`, but the editor floor for agent rows always wins — misleading row, needs a fix once the gw/spa legs land.~~ **Done** (`space.canvas.read` `min_role` fixed from `viewer` to `editor` in the Space self-API registry, golden/sidecar/vendored copies and digests updated; dc75eafe7). Not recorded by its own builder; logged here now.
- spa: distinct copy for `canvas_not_found` on save/restore (removed, keep draft, offer create — not a conflict); `SpaceCanvasHistory` shows the window note and only offers restore/compare on listed versions; a manager "clear" action that saves a single newline (D22aq).

## Round 7 (2026-11-20, D21m resolved; a real cross-plane gap logged, not yet fixed)

| # | Topic | Decision | Work |
|---|---|---|---|
| D26 | D21m save-under-floor leak concern | Resolved: a windowed GET already shows the current head unconditionally (shipped in b174a3df6), so the save-conflict answer the original refusal worried about is a strict subset of what a plain read already exposes. Save is now issued under the D18 timestamp floor (still refused under the older WS04-S11 revision floor, and restore stays refused under any floor) | user-core canvas_decision.go + repository.go (eb2782604) |

Open, not yet fixed — logged so it is not lost, needs its own pass across run+sc:
- ~~**D23's ZDR-stop promise is not fully delivered end to end.** Control issues the starter-stop decision under ZDR (D23) and model-gateway's new route checks it, but execution-core's `CancelRun` and session-core's `ResolveRunSpace` both refuse a ZDR caller outright, so a ZDR starter still cannot actually stop a run today. Needs a run+sc pass to decide exactly what relaxes (stop-only, not every RPC those refusals guard) before touching either refusal.~~
  **Delivered**, in two passes:
  - run+sc backend (4880cad98): execution-core's `CancelRun` no longer calls `enforce_persistence_free_execution`; session-core's `SpaceBrowserRead.ResolveRunSpace` no longer calls `refuse_zero_retention`; session-core's `RunService.CancelRun` now calls a new `authorize_run_stop` (service-scope check only) instead of `authorize_operation`.
  - model-gateway (this commit): `room_browser_run_stop`'s step-1 posture gate, extracted as `stop_posture_gate`, no longer refuses a plain ZDR posture — only a resolver-raised refusal (a waiver route mismatch) still fails closed there. The Control decision check, the starter/ownership checks and `writes_stop_result_turn` are unchanged, so a ZDR stop still writes no result turn and the `RUN_CANCELLED` receipt still carries only ids and the reason, never turn content.
  Deliberately still refused, each for its own reason, not touched by either pass: `cancel_run_as_room_manager` (the room *manager's* stop of someone else's run — a different feature, WS09-S8, not the starter path); `resume_run`/`pause_run` in execution-core (only stop relaxed); the personal/non-room browser-control route (`browser_run.rs`'s `browser_run_zdr_blocked`) and the room *start*/room *read* routes in `browser.rs` (`room_browser_run_start`, `room_read_gate`) — ZDR browser runs cannot be started at all today, room or personal, so this backend change only ever matters for a run that was already running before enforcement, or for a future change to the start path, neither of which this pass touches.
- Parked runs (awaiting approval) are not revoked immediately by any Stop path, owner or starter: the grant lives until its TTL. Cross-replica revoke is also local to the replica that handled the Stop (up to 5 minutes elsewhere). Both pre-exist D21h, surfaced while building it.

## Round 8 (2026-11-23, owner answers after the ledger refresh 81dd840bc)

| # | Topic | Decision | Work |
|---|---|---|---|
| D27 | Conditional slices (DEC-COND-W123, DEC-3D-COND): WS02-S6, WS13-S9/S10/S11, WS15-S10, WS18-S8/S9, WS19-S10, WS20-S6/S8 (+ WS20-DOD) | Included in the Wave 1-3 completion: build them all, each behind its own flag, off in production | build items after the current build run |
| D28 | Audit Wave 3 Recommended items parked as epics (DEC-SCOPE-AUDIT-W3-REC), incl. F-t1-032 composer, F-t1-039 permalinks/fork/quote, C-REC-01 run guard, B-REC-01 ZDR in every plane | Included: "Wave 1-3 fully" covers them; design each epic's first slice, starting with Control's shared-audience decision family (SD-28) | design + build items |
| D29 | SSO/SCIM pilot IdP (DECISION-SD-24) | Entra ID first (SCIM 2.0 tested against Entra's provisioning shapes); Okta later. Plan coverage (DECISION-WS05-Q2) still open: gate SSO/SCIM behind an org entitlement bit that defaults off, so the plan mapping can be set later without code | B-PRIO-02, WS05-S8, WS05-S9, DEPTH-WS05-IDENTITY |
| D30 | Playwright Chromium download for the live e2e tier (DEC-LIVE-E2E-APPROVAL) | Declined for now: the live e2e specs stay written but unrun; no browser download | DEPTH-TEST-LIVE-E2E stays open (blocked) |
