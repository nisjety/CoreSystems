
## 5. Dependencies and ordering

### 5.1 Dependency table

| Slice | Hard dependencies | Soft dependencies | What it unblocks |
|---|---|---|---|
| 1 Omnibox | none to build or merge | D4 slice S-1 (org in the gateway cache key) and S-2 (`zdr` forwarded on search): production flag `VITE_BROWSER_OMNIBOX_SEARCH` stays off until S-1 merges. WS8 (D8): without it every cross-host click is refused and the session dies after 12 actions (F1) | Slice 4c |
| 2 Reload | Quarry edge deployed before the gateway lists `hard_reload` | D3 slice S3 (`chromiumoxide.rs` order, D3 section 5), D2 slice S2 if the file split has landed | none |
| 3a Quarry filters and proxies | none | D2 slice S2 (`agent_routes.rs` split: rebase) | 3b, 3c, 6a |
| 3b Panels | 3a | none | none |
| 3c `browser.console` | 3a; for end-to-end evidence D1 (grants the broker can validate) | sibling (b) owns the execution-core edit sequence | F-t2-028 (classification fix) |
| 4a Controller | none | none | 4c, Space reuse |
| 4b Dock layouts | none | the in-flight chat edits in `ChatPage.tsx` [M] | 4c |
| 4c Interactive dock | 4a, Slice 1; for general availability WS8 (human budgets and cross-host policy) | D4 (ZDR session behaviour) | 5 |
| 5 Control strip and tab markers | D2 slice S3 (epoch, `expected_epoch`, `initiatedBy`, `USER_TOOK_OVER`) | `openedBy` from WS8 or D2 (D-19) | F-t2-038 |
| 6a Observed live view | Model Plane half (sibling (b) and WS8): execution-core run-to-session record, unary frame and evidence RPCs, model-gateway relay routes | 3a (shared projections), D1 (grants) | F-t2-038 presence, F-t2-028 |
| 6b, 7 Attach and agent tabs | ADR topic 3, D1 session-bound child grants, D2 `StepInitiator`, a delegation claim | audit open question 12 (long-lived run per chat thread) | F-t2-024, F-t2-038 |

### 5.2 Build order and parallelism

Wave A, parallel and file-disjoint apart from `BrowserChrome.tsx` wiring hunks: Slice 1, Slice 3a, Slice 4a, Slice 4b (and the start of 3c with sibling (b)). Wave B: Slice 2 (after D3 S3), 3b, 4c, and Slice 5 once D2 S3 is deployed. Wave C: Slice 6a. Slices 6b and 7 wait for the ADR. Parallelisable with other workstreams: D3 (except `chromiumoxide.rs`), D4 slices S-1 to S-4, D5, anything not touching `BrowserChrome.tsx`, `browser.rs` or `agent_routes.rs`. Merge order proposal across workstreams: D1 broker work first (its own rule), D2 S1 then S2 (file split) then S3, then D9 Slices 3a and 5, with the SPA slices free to land at any point because they only touch new files plus wiring hunks.

### 5.3 Merge-conflict hotspots (files and line ranges shared with other workstreams)

| File | Ranges D9 touches | Shared with |
|---|---|---|
| `FE/src/features/dashboard/home/BrowserChrome.tsx` [C] | L846-851 and L1166-1212 (Slices 1, 2), L199 and L1907-2004 (3b), L1073-1100 and L1220-1245 (5), L2026-2060 (3b) | F-t2-028 (L811 click translation), F-t2-033 (L322-395 and L2039-2056), F-t2-038 (L413, L1822-1857), D3 slice S9 (error copy), sibling (a) presence work |
| `GW/domains/browser.rs` [C] | L284-287 and L761-800 (5), L1784-1850 (3a), L2578 and L2700 (2, 3a), L3702-3720 (2), L3800-3910 helpers (visibility, 3a), L3873 (1) | D1 (L451-565, L1212-1382), D2 (L663-760, L3873 `USER_TOOK_OVER`), WS8 (L483-495 literals), D3 S9 |
| `Q/crates/quarry-edge/src/agent_routes.rs` [C] | L1061-1066, L3454-3510 (3a), action labels near L695 and L752 (2) | D2 (the whole file until slice S2), F-t2-008, 011, 014, 021, 032, 036 |
| `Q/crates/quarry-browser/src/chromiumoxide.rs` [C] | one hunk near L3044 (2) | D3 slices S2, S3, S5, S10, S6 (serial order), F-t2-020, 021, 023, 034 |
| `Q/crates/quarry-core/src/contracts.rs`, `quarry-runtime/src/observation.rs` [C] | `AgentAction::Reload` L824, observation L260, L340, L545 | D3 S7 and S9, F-t2-015, 021, 034 |
| `Q/docs/openapi.yaml` [M] | `AgentAction`, devtools path | D2, D3, D5 |
| `GW/domains/search.rs` [C] | L524 (suggestions) and visibility of `search_web` | D4 slices S-1 and S-2 (L195-231 region) |
| `FE/src/features/chat/components/ChatPage.tsx` [M], `use-chat-controller.ts` [M] | L753-755, L973-984; L343-348 | sibling (a) and (b) chat edits; the 7 uncommitted hunks |
| `FE/src/features/chat/components/ChatLiveRunPanel.tsx` [C], `chat-run-watch.ts` [C] | one slot prop; L410-452 and L467-470 (6a) | F-t2-013, 014, 028, 038 (sweep S7 hotspot 4) |
| `FE/src/shared/api/browser-client.ts` [C], `action-registry.ts` [C], `GW/domains/actions/handlers.rs` and `dispatchers.rs` [C], `errors.ts` [M] | additive types, ids and rows | D3 S9, WS8, F-t2-032 |
| `FE/src/shared/session/home-local-content.ts` [C] | one key and purge call | sibling (a) if it adds Space local keys |
| `MP/rust/services/execution-core/src/runtime_loop/agent.rs` [M], `grpc.rs` [M], `runtime_loop/mod.rs` [M], `tool_bridge/mod.rs`, `provenance.rs`, `model-gateway/src/moderation.rs` | L4204; L862-870; L45-46 and L604; L429-470; L63-66; L235-237 | sibling (b): F-t2-033 approval continuation (agent.rs L4566+), F-t2-028, F-t2-045 |
| `FE/src/styles/global.css` [M] | none by design | every UI workstream |

## 6. Plane ownership, ZDR propagation and Model-proposes/Quarry-executes check

- Control Plane owns identity, orgs, billing, sessions, audit, quotas, entitlements. D9 adds none and reads none beyond the verified session: the gateway derives org and user from `require_session` and `resolve_browser_run_owner` (never from `x-verevon-org-id`, enforced by the fabrication guard). OK.
- Data Plane v2 owns documents, chunks, embeddings, retrieval. D9 persists nothing there; omnibox history and dock state are local browser storage; procedures and receipts are Quarry-owned and are only read. No embeddings, no reranking. OK.
- Ingestion Plane captures evidence through contracts only. D9 reads Quarry receipts, egress receipts, devtools and procedures through Quarry's HTTP surface using the user's owner-scoped token; it never reads Quarry's database, and the Model Plane relay in Slice 6a uses execution-core's own service token for its own lease. OK.
- Provider actions through integration-corev2: not touched. OK.
- Model Plane owns reasoning, sessions, runs, capabilities, sandboxes, browser grants and cost. `browser.console` and the binding contract are Model Plane work (sibling (b) or WS8); the Verevon gateway only proxies after session-core's owner proof (`owned_orchestration_run`). OK.
- Application Plane owns collaborative projections and notifications: untouched here; presence and notifications are F-t2-033 and F-t2-038 and sibling (a). OK.
- Frontend Plane owns presentation and same-origin normalisation: omnibox, dock, strip, panels, proxies and projections (typed envelopes `{data}` and `{error:{code,message,details}}`, no upstream secrets, no raw OAuth tokens, no forged scoping headers: the gateway never forwards a client-supplied actor beyond the existing tighten-only field). OK.
- No cross-plane database access: none proposed. The tempting variant "teach Quarry to admit org members to service-owned runs" is rejected in D-13.
- The Model proposes, Quarry executes or rejects: every human action in this design (navigate from the omnibox, reload, click, take over) is a request that Quarry validates against the run's grant and domain list; `reload` is a human-only wire action the Model tool schemas do not contain; `browser.console` is read-only and grant-checked; page context never travels from the SPA to the Model (the Ask paths attach nothing; F-t2-028 is the governed route). OK.
- ZDR propagation. Omnibox history: none under ZDR, enforced client side (retention verdict, temporary thread, ZDR session) and suggestions refused server side (`request_zdr`, `org_zdr_enabled`). Dock state: layout only, client side, never server side, no content. Evidence panels: receipts and procedures are unavailable under ZDR (Quarry writes none) and the UI says so; egress receipts are readable live only and never stored client side; nothing from panels is persisted in the SPA. Model tool: console output is not persisted by D9 and model-gateway already refuses ZDR browser runs. Free-text search under ZDR goes through the gateway search path, which D4 makes ZDR-aware (no cache, no `SearchIssued`). The binding in Slice 6a refuses ZDR runs. OK, with the dependency on D4 recorded.
- Rules at risk, stated plainly: (1) the gateway search cache without an org key (F3) is a cross-tenant exposure that the omnibox would widen; mitigated by the production flag and D4 S-1. (2) `browser.observe` and `browser.act` outputs are not classified as external content today (F5); fixed with Slice 3c, but it is a security fix that should not wait for the console tool. (3) Org-scoped autocomplete can show colleagues' query terms (D-4, question 2). (4) Typed text in receipts and procedures until F-t2-021: redacted at the gateway as defence in depth, not as the fix. (5) A reload re-submits a POST (D-7), human-only. (6) The 12-step human budget is a product risk, not a rule violation (F1).

## 7. Cross-session interfaces

**(a) Verevon Space (shared workspaces, agent PC or browser in Spaces, ZDR-safe telemetry).**

| Shared piece | Proposed owner | Notes and what the user must reconcile |
|---|---|---|
| `FE/src/features/browser/*` (controller, dock, omnibox, evidence panels) | WS9 (this design) | A Space-hosted agent browser should reuse `useBrowserSessionController` and the panels; Space owns where they mount. The user should confirm that Space does not fork a second browser controller. |
| Presence rendering (agent working, you are in control) | Space session renders; WS9 supplies `BrowserControlStrip` and the epoch data | Both use Quarry's `controlEpoch` and `initiatedBy` (D2), not gateway-local state. |
| Space members viewing a member's browser | blocked | `caller_owns_live_run` admits only the owner (D2 section 8); the observed relay (6a) is owner-gated and must not be widened by Space grants. |
| Local keys and `home-local-content.ts`, `ChatPage.tsx`, `App.tsx` | shared hotspot | Per-user keys follow the `verevon.<feature>.v1:{org}:{user}` pattern; Space should register its purge in the same function. |
| ZDR-safe telemetry | WS9 emits none from the SPA; gateway logs only ids and codes | No URL, title or query in logs; matches D4's log-field discipline. |

**(b) Model Plane and chat or agents.**

| Shared piece | Proposed owner | Notes |
|---|---|---|
| `browser.console` tool and the `TrustClass` fix for `browser.observe`, `browser.act`, `browser.console` | sibling (b) implements; WS9 specifies (D-16, Slice 3c) | Both copies of `classify` must change together; no ADR. |
| Binding of an orchestration run to a live session (events or RPCs, run-to-session record) | WS8 with sibling (b); WS9 supplies the SPA and gateway halves and the contract names in Slice 6a | ADR topic 3 for attach (6b); 6a is open. |
| `initiated_by: "agent"` on every Model Plane Quarry call and mapping `USER_TOOK_OVER` to a suspended run | sibling (b) (D2 section 7) | If delegated bearer forwarding makes agent steps carry a user token, principal derivation would call them human. The dock's Take over relies on this. |
| Chat controller edits (`use-chat-controller.ts`, `ChatPage.tsx`) and the floating composer's submit path | sibling (b) owns the controller; WS9 adds a 6-line dock hook | Agree an edit order; the floating composer may need a lifted composer-settings snapshot (D-12). |
| Chat-thread versus Quarry run lifetime (audit open question 12) | user decision | Decides "keep chat" semantics, 6b and F-t2-054. |
| Approvals (F-t2-033) | sibling (b) | The shared approval card (epic) should live in `features/browser` or chat; decide once. |

What the user must reconcile across both: who edits `ChatPage.tsx`, `ChatLiveRunPanel.tsx`, `BrowserChrome.tsx` and in what order; whether `src/features/browser/` is the single home for browser UI; and the audit topic 3 decision that gates attach.

## 8. What can proceed WITHOUT an ADR or decision, and what is blocked

Proceeds now, no decision needed: Slice 1 (code and merge; production flag held by D4 S-1), Slice 2, Slice 3a, Slice 3b, Slice 3c (the unit-level work and the classification fix; end-to-end evidence needs D1), Slice 4a, Slice 4b, Slice 4c code behind its flag, Slice 5 once D2 S3 is deployed, Slice 6a's Verevon halves once the Model Plane contract names are agreed (a coordination step, not an ADR).

Blocked: general availability of Slices 1 and 4c on WS8 (human-session budgets and cross-host policy; audit topic 3 and open question 2); Slice 6b and Slice 7 on audit topic 3 (shared human and agent sessions, delegation claim); the production omnibox search flag on D4 S-1; end-to-end evidence for 3c on D1; F-t2-033, 028 and 038 on their own dependencies (F-t2-001, F-t2-032, Slice 5, Slice 6).

ADR-GATED summary: topic 3 (Slices 4c GA, 6b, 7, D-13), topic 4 (D-3 ZDR per feature; recommended default matches D4), topic 8 (not needed for the first slice; the later envelope work is outside this design), topic 1 (not touched; policy denylist for human browsing is WS8's).

## 9. Open questions for the user (only real blockers)

1. Is it acceptable to land Slices 1 and 4c behind flags and leave them off in production until WS8 removes the 12-step, first-host, 120-second session limits? Without WS8 the omnibox works for one host for 12 actions. (Blocks GA, not the work.)
2. Should the omnibox suggest org-scoped autocomplete terms (colleagues' searches) like the dashboard dropdown does, or only local history plus related queries (D-4)? This is a privacy call.
3. Default for "new chat per tab or keep the chat": keep (recommended) or new (Aside's default)? (D-11)
4. Shared human and agent session design (audit topic 3, open question 12): attach to the user's session, or observe-only relay plus separate agent session (D-13)? Slice 6b and 7 cannot be scheduled until this is answered; 6a assumes the answer "observe first".
5. Confirm that `browser.observe`, `browser.act` and `browser.console` outputs should be classified as external browser content now (a security fix, F5), and that sibling (b) owns the execution-core edit.
6. Confirm that a ZDR thread's dock is a human-only browser (no AI run; model-gateway already refuses ZDR browser runs) and that the product copy for that is acceptable.

## 10. INTERFACE SUMMARY

**Files touched.** Frontend: `src/features/browser/{omnibox,evidence,dock,session}/*` [NEW]; `BrowserChrome.tsx`, `ChatLiveRunPanel.tsx`, `chat-run-watch.ts`, `browser-client.ts`, `action-registry.ts`, `home-local-content.ts`, `use-chat-shortcuts.ts` [C]; `browser-omnibox.ts`, `ChatPage.tsx`, `use-chat-controller.ts`, `KnowledgeComposer.tsx`, `errors.ts` [M]. Gateway: new `browser_evidence.rs`, `actions/browser_omnibox.rs`; edits in `browser.rs`, `search.rs`, `actions.rs`, `handlers.rs`, `main.rs`. Quarry: `agent_routes.rs`, `contracts.rs`, `actions.rs`, `lib.rs`, `chromiumoxide.rs`, `observation.rs`, `action_runtime.rs`, `openapi.yaml`. Model Plane (sibling b): `tool_bridge`, `quarry_agent`, `runtime_loop`, `provenance`, `moderation`, `capability_policy`, `grpc`.

**New contracts.** Wire action `reload{ignore_cache}`; devtools params `levels, filter, category, tabId` plus `nextSequence`; gateway routes `.../receipts`, `.../egress-receipts`, `.../procedure`, later `/runs/{id}/live*`; action ids `browser_omnibox.search`, `browser_omnibox.suggest`, `browser_evidence.compile_procedure`; errors `browser_navigation_outside_scope`, `browser_receipts_unavailable_zdr`, `browser_procedure_not_compilable`, `browser_control_epoch_stale`; tool `browser.console`; keys `verevon.browser-omnibox.v1`, `verevon.chat.browser-dock.v1`; flags `VITE_BROWSER_OMNIBOX_SEARCH`, `VITE_CHAT_BROWSER_DOCK`; capabilities `hard_reload`, `evidence`. No SQL, no proto in scheduled slices (6a proposes unary RPCs).

**Dependencies.** Hard: D2 S3 (Slice 5), D1 (3c evidence), Model Plane half (6a). Soft: D4 S-1 and S-2, D3 S3, WS8 for usefulness.

**Effort.** S1 5 d, S2 4 d, S3a 5 d, S3b 4 d, S3c 4 d, S4a 4 d, S4b 5 d, S4c 4 d, S5 3 d, S6a 8 to 10 d; about 47 d scheduled. 6b and 7 unscheduled (L to XL).

**ADR gates.** Topic 3 (4c GA, 6b, 7), topic 4 (omnibox ZDR).

**Parallelisable with** D3, D4, D5 and Slices 1, 3a, 4a, 4b together. **Conflicts with** `BrowserChrome.tsx` (F-t2-028, 033, 038), `browser.rs` and `agent_routes.rs` (D1, D2, WS8), `ChatPage.tsx` (siblings), `chromiumoxide.rs` (D3).

**Audit corrections.** Browser MCP tools are not classified untrusted; `search.rs:100-140` and `browser.rs:3310` are drifted; the existing cross-host Playwright spec contradicts the first-host session limit.

<!-- COMPLETE -->
