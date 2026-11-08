# 5. Later waves as epics (audit W3, W4, W5)

Source: audit sections 9.1 (W3, W4, W5 tables), 9.3, 9.4, 10.1 at `C:/dev/verevon-audit-2026-10-14/03-model-plane-chat-and-agents-audit.md`; the cards of the W3/W4 items; `common/overlap-space.md` and `common/overlap-quarry.md`; the decision register in section 2 (`DEC-nn`). Prerequisites are named as `WSnn` (the 27 designs in section 3, waves W1-W2) and `DEC-nn`.

## 5.0 What this section is, and is not

- W3 (23 items) and W4 (12 items) are NOT file-level designed. Nothing below has the evidence level of a `WSnn` design: no slice list verified against the tree, no migration numbers, no proto tags. Each epic states what must be designed before the first line of code. Paths quoted are the audit cards' own citations, marked as the audit gave them; they were not re-opened for this compile. Re-verify before design.
- Effort band = the audit's verified bands of the member items, aggregated by judgement. Audit risk 10.1 #7: 30 of 40 revised estimates went up, mostly because items bundle separable deliverables. Plan every XL as the split given here, never as one unit. The aggregate is not a new estimate.
- W3/W4 items are sequenced by the audit's judgement [INF]. Four trunks gate almost everything: the durable loop (`WS03`, F-t3-002), declared tool classes (`WS05`, F-t3-003), the sandbox computer (`WS22`, F-t3-009) and the history foundation (`WS17`, F-t3-012). A slip in any of them moves the epics hanging on it (audit 9.4 #1, #4, #5; 10.1 #6).
- The 35 W3/W4 items form 14 epics; every item sits in exactly one.
- DEC-40 ("Parked") lists topics no W1-W2 design covers: share model, LLM reviewer, Channel Plane, Gemini/Vertex EU, wide-research gate, BYO metering. Epics EP-4, EP-11, EP-12 and EP-13 each depend on a DEC-40 topic that has no builder yet.
- ZDR: every epic adds new stores or egress. Each carries an explicit ZDR clause and a case in the resolver's golden matrix (audit 10.1 #2; DEC-32 for waived runs). Where the audit says an item is refused under ZDR, that refusal is a requirement, not an option.

## 5.1 Epic index

| Epic | Title | F-t3 items | Effort band (members) | Owner planes | Hard prerequisites |
|---|---|---|---|---|---|
| EP-1 | Computer track: takeover, live panel, replay, desktop | 026, 035, 050, 024 | XL (M, L, M, XL) | Model, Ingestion (Quarry), Frontend, Endpoint (parked) | WS21, WS22, WS03, WS14, DEC-19..23 |
| EP-2 | Research and citations | 054, 027, 053, 036 | XL (M, XL, M, L) | Model, Data, Frontend | WS23, WS26, WS27, WS03, DEC-02, DEC-26, DEC-40 |
| EP-3 | Artifact canvas and composer | 031, 052 | XL (XL, L) | Model, Frontend, Application, Data | WS17, WS08, WS19, DEC-28, DEC-29, DEC-37 |
| EP-4 | Inline outputs and MCP Apps | 044, 045 | L (M, L) | Frontend, Model, Ingestion, Application, Control | EP-3, WS05, WS14, WS26, DEC-40 (origin) |
| EP-5 | Decks and images as deliverables | 038, 051 | XL (L, XL) | Model, Frontend, Data, Control | WS19, WS08, EP-2, EP-13 (Gemini) |
| EP-6 | Agents and Automations hubs | 019 | XL (phase 1 is M) | Application (definitions), Model, Frontend | WS20, WS24, DEC-14, DEC-16, DEC-17 |
| EP-7 | Unattended autonomy: goals and event triggers | 037, 047 | XL (XL, XL; trimmed 047 is L) | Model, Ingestion, Control | WS20, WS03, WS27, DEC-02, DEC-16, DEC-18 |
| EP-8 | Scoped memory and personalisation | 060, 061 | L (L, L) | Model (session-core), Control, Frontend | WS07, WS02, WS18, DEC-28, DEC-30, DEC-32 |
| EP-9 | Policy, egress and compliance | 043, 055, 062, 068 | XL (XL, L, XL, L) | Control, Model, Ingestion, Support, Frontend | WS25, WS22, WS06, WS01, WS10, DEC-14, DEC-19, DEC-21 |
| EP-10 | Connectors: suggestion, write confirmation, custom | 056, 057 | L (L, L) | Model, Ingestion (integration-corev2), Frontend | WS23, WS05, WS13, DEC-21, DEC-26, DEC-27 |
| EP-11 | Authored behaviours and plugins | 058, 059 | XL (XL, L) | Model (capability-core, session-core), Control, Frontend | WS19, WS07, WS25, DEC-30, DEC-40 (supply chain) |
| EP-12 | Sharing, export and the public Agent API | 046, 064 | XL (XL, L) | Model, Control, (Channel docs-only) | WS17, WS04, WS03, WS12, WS15, DEC-17, DEC-40 (share model) |
| EP-13 | Model record, providers, Gemini and BYO | 042, 041, 065 | XL (L, L, XL) | Model (inference-core, session-core), Control, Frontend | WS10, WS11, WS27, DEC-13, DEC-21, DEC-33, DEC-40 |
| EP-14 | Run legibility, trajectory ledger and release gates | 034, 048, 040 | XL (XL, M, L) | Model, Frontend | WS15, WS09, WS12, WS01, EP-13 (for 040) |

Suggested order, not a schedule: start EP-13 slice 1, EP-14 (048 and 040 slice), EP-8 slice 1, EP-2 slice 1 and EP-6 phase 1 as soon as their W1-W2 prerequisites are green; they need little new infrastructure. EP-1 and EP-9 are decision-bound. EP-7, EP-12 and EP-5 wait on the trunks.

---

## 5.2 Epics

### EP-1 Computer track: takeover, live panel, replay, desktop
- Items: F-t3-026 (takeover and hand-back, M), F-t3-035 (live computer panel with typed viewers and PTY exec, L), F-t3-050 (scrubbable browser-run replay, M), F-t3-024 (computer use in the sandbox desktop, XL).
- Owner: Model Plane (pause and resume, tool surface, lease-bound guest executor), Ingestion Plane (Quarry owns control mode, enforcement and epoch), Frontend Plane (`ChatLiveRunPanel`, takeover card), Endpoint Plane only if user-desktop control is ever unparked (DEC-22 parks it; do not plan it).
- Prerequisites: `WS21` (agent browser on real grants; S5a/S5b Do tools, S6 consent card and live-panel binding, S9 interim human login hand-off), `WS22` (S6 typed lifecycle and `sandbox_state`; S7 SandboxBackend trait and `SandboxFailureCause`, which WS22 lists as the seam for F-t3-024/035), `WS03` (takeover reuses `/continue` and the single-use approval consume), `WS14` (`QuestionCard` primitive; browser `human_assist` uses approvals, not questions), `WS16` (human-assist modelled as approval rows so it surfaces in the attention rail), DEC-19 (site policy ownership), DEC-20 (browser Do tools server-bound), DEC-21 (secret custody), DEC-22 (sandbox backend reversal, computer-use scope), DEC-23 (thread workspace). Audit 9.4 #4: the order is strict: F-t3-008 then 026; F-t3-009 then 035 then 024. Nearly all XL cost of 024 sits in F-t3-009 (`WS22`) and must not be counted twice.
- Cross-document: overlap-quarry C4: Quarry's own takeover enforcement (initiator from principal, `USER_TOOK_OVER`, control epoch; F-t2-013 phase 1) must land before 026, because the audit's premise "Quarry already covers it" is false until then. Quarry plan owns enforcement and epoch; Model plan owns the request/resume card; attach/delegation is a Model Plane grant child (overlap-quarry A4). Overlap-quarry A3: split `ChatLiveRunPanel`/`ChatPage` (this epic) from `BrowserChrome` (Quarry plan); both consume one `BrowserSessionViewModel`.
- First slice (S-M): wire the existing manual takeover toggle. Per the card, `ChatLiveRunPanel` mounts `BrowserChrome` with pause and resume but passes no `onBrowserControlMode`, so the toggle is disabled; the BFF already returns 409 to agent actions while a human holds control (audit citations `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/browser.rs:690-715`, Quarry `step_receipts.rs:55-95`). Then 026 proper: `human_assist` approval kind from `ChallengeSignal`, resume with the user's note and a fresh snapshot; human-typed input never reaches the model.
- Slice order: (1) 026 takeover; (2) 050 slider and per-action brief over existing events (`browser_events.rs` dispatched/decided/observation with `screenshot_ref`); (3) 035 viewer registry keyed on ToolIntent (browser, process, file kinds) plus the missing human output read path for the read-only terminal; (4) 024 step 1 (typed connector, screen hierarchy into the Do prompt and capability policy); desktop surface only after 009 and 035; coordinate fallback stays off until an ADR.
- Top risks: (1) authority creep and prompt injection: on-screen text in a guest desktop, login carry-over after takeover (needs an expiry), `capability_policy` fails closed on an unmapped tool id so computer use needs a new capability id; (2) ZDR: terminal output, recordings and the process registry become new stores; no clip exists under ZDR by construction (050); retention and redaction policy must precede 050; (3) vision cost and a strictly ordered chain: a slip in `WS22` or `WS21` moves all four items.
- Pull forward: 050's scrubber alone is S (events and screenshots exist; audit quick win); it can follow `WS21` S6. Read-only terminal viewer can follow `WS22` S6 once the process-output read path exists. The takeover toggle wiring (above) is S and belongs to `WS21` S6.
- Must be designed before starting: the 026 `human_assist` kind (one approval-kind registry with `WS13`/`WS14`/`WS16`); the viewer-registry contract and redacted-output read path (035); the executor protocol and guest-agent lifecycle for 024, which needs the DEC-22 ADR and the S8 microVM spike. 024 is not startable until DEC-22 closes.

### EP-2 Research and citations
- Items: F-t3-054 (web reading upgrade, M), F-t3-027 (deep research v2, XL), F-t3-053 (claim-level citations, M), F-t3-036 (wide research, L).
- Owner: Model Plane (model-gateway `deep_research.rs`/`tool_loop.rs`, execution-core `subagent/research_fan_out.rs`), Data Plane (knowledge sources, passage viewer through the Data Plane read API), Frontend Plane (plan card, passage viewer, `ChatWideResearchPanel`). Quarry owns the scrape contract and SSRF vetting; Model owns the tools and research runs (overlap-quarry lesser overlaps).
- Prerequisites: `WS23` (connector reads; F-t3-027 needs F-t3-010), `WS26` (content safety; every fetched chunk is untrusted and screened; F-t3-033 should land before F-t3-054, audit 10.1 #4), `WS27` (cost meter and estimate; F-t3-036 depends on F-t3-028), `WS03`/`WS04` (durable loop and replica-safe control for run-based fan-out; 036 depends on F-t3-002), `WS02` (cache and context), DEC-02 (the 300 s delegated bearer bounds fan-out scale until a renewable bearer), DEC-26 and DEC-32 (connector reads, ZDR on connector/web reads), DEC-40 (OD7 wide-research spend gate: recommendation is a separate child budget and no confirm gate in v1), DEC-18 (cost semantics).
- First slice: 054 (M, no audit dependencies): outline plus first chunk plus reference id instead of the 4,000-character cut (`MAX_FETCH_CHARS` at `tool_loop.rs:124-126`), `read_page` and `find_in_page` over the Quarry artifact id the gateway already resolves, batched `web_search` with stable reference ids. Overlap-quarry D10: Quarry's inline return (F-t2-005) precedes the ZDR buffer path; the non-ZDR artifact path works today.
- Slice order: 054; 053 chip binding (the client drops the server's `number`, `chat-client.ts:953-969`; this half is client-only); 027 in three slices (editable plan turn then run turn re-clamped server-side; in-process second pass driven by the coverage shortfall inside the wall clock; knowledge and connector sources under viewer-ACL retrieval); 036 entry point, estimate card, spend check, per-item retry, replay after reload. 027 mid-run steering is demoted: queued input lands only at tool-round boundaries.
- Top risks: (1) the pipeline dies with its SSE stream, so no plan or spend gate can wait inside it; moving research onto a durable run (the `WS03` fold) is a decision, not a refactor; (2) ZDR: deep research is refused under ZDR, so no claim ledger persists there; connector and web reads in non-ZDR runs persist truncated tool output into step records, so F8 redaction (`WS23` S1) must land before connector sources (audit 9.3 #10); (3) cost: two-pass research, N delegated children and unmetered fan-out (cap 16 today); the org cap must clamp client-requested counts server-side.
- Pull forward: 054 immediately after `WS26` (it has no dependencies). 053 chip-binding half needs no research epic. 036's composer entry point and estimate card can follow `WS27` S3/S4 without waiting for 027.
- Must be designed before starting: where research state lives (run record vs session-core fold vs in-memory); the claim-to-source ledger schema and its erase contract; `knowledge_only` scope is rejected with deep research today (`sse.rs:945-985` per the card), so source-scope semantics need a design; the OD7 spend gate.

### EP-3 Artifact canvas and composer
- Items: F-t3-031 (artifact canvas, XL), F-t3-052 (typed context tokens, drafts, thread references, L).
- Owner: Model Plane (model-gateway `artifacts.rs` revision store and routes), Frontend Plane (Output panel editor, composer), Application Plane and Data Plane (collaborative documents and "save to knowledge" as a Data Plane document under the user's token, later phase).
- Prerequisites: `WS17` (F-t3-012 history: paging and by-id read, `recall_conversations` for thread references, message tree), `WS08` (attachments v2 for typed tokens), `WS19` (FE renderer registry S1; Office previewer S5 shares the viewer registry), `WS18` (Spaces as project), DEC-28 and DEC-29 (OD19: precedence, branch semantics), DEC-37 (OD18 caps).
- [RECONCILE] Two versioned-document stores: Model Plane artifact revisions (thread-owned; the gateway already owns the version counter and `lock_commit`/`unchanged_from`) versus the Data-owned canvas with expected-head CAS in the Space audit (F-t1-014). Proposed owner in overlap-space row 13: artifact revisions are Model Plane; Data owns authored docs and collaborative documents. Settle in design, not by default.
- First slice (M): the server-side revision store with ids, a read/write/restore route and editor attribution; the version stepper stays read-only until it exists. Then user editing in the Output panel, selection comments and selection-scoped AI edits; annotation; collaborative documents last (own design).
- 052 first slice (M, FE-mostly): per-thread text drafts on the chat page (only Home persists a draft today, `home-draft-storage.ts`); typed tokens in the send payload next; thread references through read-only outline/search/read tools last.
- Top risks: (1) `@` is reserved for Space-governed agent mentions: F-t3-052 must use another trigger; refusal is explicit reversal, not silent (audit 10.1 #9; `WS23` D6); (2) ZDR threads persist no artifacts and refuse thread references; referenced thread content is untrusted data and Space access never widens an owner-plane ACL; (3) the two-store conflict above and the persisted-message-version question from `WS17` (versions were rejected once; audit 10.1 #9).
- Pull forward: drafts per chat thread (S, FE-only, top frustration) can ship with `WS17` S2. Revision store can start once `WS17` S3 lands.
- Must be designed before starting: the revision store schema and erase contract; the tokens wire format and who authors slash templates (OD19 part); the collaboration model if scheduled.

### EP-4 Inline outputs and MCP Apps
- Items: F-t3-044 (inline interactive outputs, M), F-t3-045 (MCP Apps, elicitation forms, hosted artifact runtime, L).
- Owner: Frontend Plane (`sandbox-html.ts`, transcript widgets), Model Plane (MCP client in model-gateway: `mcp_jsonrpc.rs`, `mcp_http.rs`), Ingestion Plane (attributed image results), Application Plane and Control Plane (only if a dedicated user-content origin is ever approved).
- Prerequisites: EP-3 (F-t3-031, F-t3-052), `WS05` (F-t3-003: MCP `tools/call` passes the capability gate), `WS14` (`CreateRunQuestion` and `QuestionCard` are the reusable human-input primitive for MCP elicitation, per `WS14`), `WS26` (widgets and MCP Apps enlarge the injection surface), `WS13`, DEC-12 (MCP envelope), DEC-40 (user-content origin and share model; recommendation: MCP Apps on the existing sandboxed frame before any new origin).
- First slice (M): inline HTML/SVG widgets in the transcript reusing the opaque-origin sandbox (`allow-scripts`, no `allow-same-origin`, `connect-src none`) with host theme tokens and a narrow postMessage bridge whose follow-ups appear as visible, rate-limited user turns. Selection-to-context and the attributed image strip are separate later pieces.
- 045 slice order: elicitation forms reusing the `WS14` pause; MCP Apps `ui://` resources inside the existing frame with JSON-RPC over postMessage; every app-initiated call attributed to the viewer, never the author. The dedicated user-content origin, viewer-bound connectors and key-value store are deferred.
- Top risks: (1) a grep found no `ui://`, `resourceUri`, `elicitation/create` or `sampling/createMessage` support; the MCP client declares empty capabilities and opens a fresh session per call, so session lifetime is net-new design; (2) authority: a widget `tools/call` must reach approvals and the signed gate; viewer-bound, never author-bound; (3) ZDR: no widget snapshots, no persistence or key-value store under ZDR; thumbnails with links only.
- Pull forward: 044's bridge-less static widget render is S after EP-3 slice 1. Elicitation forms could follow `WS14` S-slices if the MCP session design is done.
- Must be designed before starting: MCP session lifetime and capability negotiation; the postMessage bridge contract and rate limits; the user-content origin ADR (defer it explicitly).

### EP-5 Decks and images as deliverables
- Items: F-t3-038 (deck pipeline with brand templates, L; XL with a sandbox renderer), F-t3-051 (images as a governed agent tool, XL).
- Owner: Model Plane (deck manifest and validation; image tool loop), Frontend Plane (slide strip, present mode), Data Plane (org template library as skills, research-to-deck), Control Plane (image pricing, entitlements).
- Prerequisites: `WS19` (verified Office deliverables: `mp-contracts::office_verification`, the `pptx` bundle, `skill_bundles` tables, `skill_file` contract, LibreOffice layer S4 for render; `WS19` names F-t3-038 as the consumer), `WS08` (attachments), EP-2 (F-t3-027 for research-to-deck), EP-13 (F-t3-041 Gemini for the second image provider and the F-t3-051 dependency), `WS27` (image generation is not in the cost ledger), `WS11`, DEC-37 (caps, fonts).
- First slice 038 (M): tool-owned versioned slide manifest with per-slide state under the existing deck path, per-slide validation reusing the `WS19` verifier, then slide strip and present mode drawn from the manifest. Org brand templates as skills last. Decks today: python-pptx in `code_interpreter` and a browser export (audit citations); no renderer in the image and the workspace is deleted per call until `WS19` S4.
- First slice 051 (L): `generate_image` exposed to the chat tool loop and the Do loop with a typed result, per-run semaphore, cost accounting and no silent fallback, on the single existing provider family. The audit says split the agent tool from the edit endpoint, mark tool, C2PA provenance and the second provider: only the latter needs Gemini.
- Top risks: (1) cost: image generation is not in the cost ledger and Model Plane has no durable binary artifact store; (2) ZDR: `generate_image` is refused on ZDR turns today; no durable manifest or template library for ZDR threads; (3) the sandbox renderer pushes 038 to XL, and a long-lived image pipeline competes with `WS22` for the same sandbox.
- Pull forward: the image tool slice (above) does not need Gemini. Deck validation is a thin extension of `WS19` S2.
- Must be designed before starting: a durable binary artifact store (where do generated images live, under which ZDR rule); the manifest schema and its ownership relative to `WS19` bundles; image pricing in cost-core.

### EP-6 Agents and Automations hubs
- Items: F-t3-019 (real Agents and Automations hubs, XL).
- Owner: Application Plane (agent definitions in Convex, ADR-0002), Model Plane (runs, capability-core cron and Control-authorised run-now), Frontend Plane (hubs).
- Prerequisites: `WS20` (scheduled runs: S3 fires endpoint and "See results", S4/S5 consent contract; `WS20` states the contract and lane are the base for Run now), `WS24` (F-t3-025 action registry for per-agent action sets), EP-8 slice 1 for named-agent memory (F-t3-060), DEC-14 (policy system of record), DEC-16 (stored consent), DEC-17 (one run-origin enum).
- [RECONCILE] Where agent definitions live: the audit and overlap-space row 14 say Application Plane Convex as the single definition store (`schema.ts` agents and Space bindings exist, unversioned, string tool names); `WS25` D1 puts the org agent policy registry in capability-core. They are different records; keep them separate and settle the join in DEC-14. A second definition store in capability-core is a named risk.
- First slice (phase 1, M): the Automations hub over the existing list and PATCH, plus a Control-authorised run-now endpoint and a fires listing in capability-core. Run now must pass the same Control fire authorisation as a cron fire and never call `StartWorkflow` directly. Fleet console: proxy behind a Quarry-audience BFF or remove; the default in overlap-quarry A12 is hide until Quarry has a fleet model.
- Phase 2: versioned definitions and a builder in Convex, consumed by Model Plane at invoke time (the persona injection already does this for Space agents), tool allowlist bound to capability ids, autonomy cap, role parameter on `subagent.task`. Phase 3: persistent named agents with scoped memory, after EP-8 and `WS20`.
- Top risks: (1) authority: which delegated token runs a named agent; a persistent agent must never widen an owner-plane ACL (audit 10.1 #3); (2) scope creep into a workflow product; (3) two schedule UIs (Settings and chat) to consolidate without regressing `WS20` guards.
- Pull forward: phase 1 as soon as `WS20` S3 and S5 are in; it adds almost no new authority.
- Must be designed before starting: the named-agent authority model; the definition schema and version semantics; the Fleet keep/remove decision.

### EP-7 Unattended autonomy: goals and event triggers
- Items: F-t3-037 (persistent goals with a token budget, XL), F-t3-047 (event-triggered automations, XL; trimmed to trigger, consumer, wake gate and budgeted run it is L).
- Owner: Model Plane (session-core Goal record and ledger, capability-core cron and Watch, orchestrator-core workflow), Ingestion Plane (normalised connector events), Control Plane (signed per-continuation and per-fire decisions).
- Prerequisites: `WS20` (S4-S8 consent contract, fire/step re-verification, guards), `WS03` (S8 parking: credential window and crash interruption), `WS27` (cost ledger and ceilings: goal budget comes from cost-core run usage), `WS02` (F-t3-001), DEC-02 (OD8 renewable bearer, a hidden prerequisite), DEC-16 (stored consent), DEC-17, DEC-18, DEC-10 (OD2 Do on the product route).
- First slice (L): the audit's attended slice for 037: a Goal record and ledger in session-core (non-ZDR only), plus a goal bar in the Agent Run Console that auto-continues while the session still holds delegated authority. Judge next (prefer the proof bundle and postcondition results, rubric fallback through inference-core), then an unattended driver modelled on `ScheduledRunSupervision` that takes fresh Control authorisation per continuation. 047 first slice: trigger record bound to one connection, Space and member implementing `FireAuthorizer`, durable JetStream consumer of `webhook_received` (new nats-provisioner binding), dedupe on `webhookEventId`, payload fetched through the integration-corev2 API and classified untrusted, run started through the scheduled-run path under budget and org quota.
- [RECONCILE] The Watch source-kind design is written twice (Space F-t1-036/047 and F-t3-047); overlap-space row 15 gives capability-core (cron plus Watch) as the single owner with Control's shared-room decision family. The current Watch model is one resource, one Space, one member and the Control validators accept personal threads only for schedule/watch decisions (overlap-space row 15): shared-room triggers wait on that Control epic.
- Top risks: (1) authority: never auto-continue across approval-required effects; a stored record is not authority; the Watch primitive deliberately refuses org-wide subscriptions; (2) the 300 s bearer: runs that pause for hours or start unattended need the OD8 renewable credential, with no service-principal shortcut; (3) event storms and cost; ZDR must come from policy because there is no user turn; goal rows are a new store.
- Pull forward: nothing before `WS20` S5. The attended goal bar can be prototyped after `WS03` S7 if the judge is stubbed.
- Must be designed before starting: the Goal state machine and budget semantics on the Subscription route (a no-op there per the card); the trigger schema and which event types exist (only three in code, no mail-received event [INF], so mail triggers need an Ingestion change first); wake-gate predicate language.

### EP-8 Scoped memory and personalisation
- Items: F-t3-060 (scoped memory for Spaces, Projects, named agents, L), F-t3-061 (personalisation that tailors itself, L).
- Owner: Model Plane (session-core `agent_memory`, dreaming, model-gateway prompt assembly), Control Plane (user settings, declassify), Frontend Plane (profile editor, onboarding step).
- Prerequisites: `WS07` (memory controls and learning governance; it ships the shared predicate helper only), `WS02` (stable/context/volatile prompt tiers: the profile belongs in the cached prefix), `WS18` (Spaces as project; F-t3-013), DEC-28, DEC-30, DEC-32.
- First slice (S): memory containment. Exclude Space-thread and `agent_run` threads from user-scope extraction (the card verifies Dreaming loads pending messages across all threads with no Space predicate, `dreaming.rs:1263-1285` as cited). Overlap-space D3: this one fix is both F-t1-012 slice 1 and F-t3-060 step 1 and must land before any personal agent, room agentic run or Spaces as project collaboration. Then the migration adding `(scope_kind, scope_id)` plus an isolation flag; overlap-space row 6 proposes one session-core migration carrying the audience predicate as well.
- 061 first slice (S, no dependencies beyond `WS02`): tone presets and timezone and locale in the prompt, then the synthesised user profile as a stable block in the cached prefix with per-turn recall after the cache breakpoint.
- Top risks: (1) cross-project leakage and three scope vocabularies (capability-core, session-core free text, audience labels) merging wrongly; (2) agent memory must never carry authority; Space-scoped rows need erasure and Space-deletion coverage; ZDR handling of agent memory needs a decision; (3) never synthesise a profile from Space-origin threads; caching is off under ZDR; profile stays user-editable and provenance-labelled.
- Pull forward: the containment slice belongs in W2 with `WS07`/`WS18` (it blocks collaboration). Tone presets and locale are independent.
- Must be designed before starting: the scope-kind vocabulary migration with Control (declassify); the profile synthesis job and its review UI.

### EP-9 Policy, egress and compliance
- Items: F-t3-043 (egress tiers and credential-injecting proxy, XL), F-t3-055 (web access governance and provider transparency, L), F-t3-062 (enterprise compliance posture, XL), F-t3-068 (policy and data-flow transparency, L).
- Owner: Control Plane (org-core policy records, signed decisions, retention and hold, audit), Model Plane (enforcement at the loops and the sandbox network boundary), Ingestion Plane (Quarry re-check), Support Plane (signed releases per the audit's plane list), Frontend Plane (admin views).
- Prerequisites: `WS25` (F-t3-032 registry; S9 enforcement; policy only tightens vs the server floor; webhook hooks deferred to F-t3-059), `WS22` (F-t3-009 for 043), `WS06` (F-t3-004; the `retentionFor(org)` seam for retention and legal hold), `WS01` (F-t3-005 ZDR attested, F-t3-039 signed release), `WS10`, `WS13`, DEC-14, DEC-15, DEC-19 (web/site policy ADR: Control org-core as record; schema owned by Quarry's F-t2-022 thin slice; overlap-quarry A6), DEC-21 (custody), DEC-33, DEC-34, DEC-35.
- [RECONCILE] Credential custody: F-t3-043 part (c) names a Control secret store; DEC-21 puts provider tokens and BYO keys in integration-corev2 custody and site passwords in the Quarry vault. One custody ADR before 043(c) and F-t3-057.
- [RECONCILE] Policy store: F-t3-055 stores web policy in org-core (read through a fail-closed endpoint, not a bearer claim) while `WS25` D1 stores the agent policy in capability-core. Both stay; the schema registry (`WS25`) takes web policy keys later.
- First slice (suggested order, separable): (1) 055 org web policy record plus loop enforcement in both web paths (inline loop and execution-core) plus the "Searched via X" line; (2) 068 effective-policy origin view for approval posture, ZDR, privacy tier, routing and budgets, and the egress catalogue with CI check; (3) 043 closed tier enum (unknown means Disabled), per-call enforcement report in tool cards, then a real network boundary and the injecting proxy; (4) 062 retention windows and legal hold sweeper (dry-run, audited, propagating to Data Plane indexes and derived memory).
- Verified gaps to close (audit cards): bwrap unshares the network only for `Disabled`, so a proxy variable is advisory; Control grants `space:egress` to any non-default claim with no org entitlement (`user-core/.../space_capability_decision.go`); `IssueSpaceCapabilityDecision` requires a personal thread, so shared-room sandbox/egress decisions cannot be issued (overlap-space row 11, Control epic "shared-Space decision family").
- Top risks: (1) a loose allow-list turns the egress proxy into an exfiltration path; any tier above off is refused for ZDR and high-risk runs; (2) hold versus GDPR erasure precedence needs a Control decision; ZDR runs hold no content, so a typed refusal; do not claim signed releases before a candidate with rollback exists; (3) admin visibility must not leak admin-only or other-tenant policy; the disclosure table is a public claim and must be generated.
- Pull forward: the `space:egress` entitlement gap is a Control-only fix [verify live]; the egress catalogue and CI check (068) fits `WS12`'s guardrail lane.
- Must be designed before starting: the web-policy ADR (DEC-19); the retention schema in org-core and the sweeper's deletion contract with `WS06`/GDPR; the egress boundary mechanism (needs `WS22`).

### EP-10 Connectors: suggestion, write confirmation, custom
- Items: F-t3-056 (suggestion cards and editable write confirmation, L), F-t3-057 (custom connectors, L).
- Owner: Model Plane (execution-core `integration_tools.rs`, model-gateway MCP registry), Ingestion Plane (integration-corev2: custody, REST executor), Frontend Plane (cards, add flow).
- Prerequisites: `WS23` (S8 approved writes; `WS23` names F-t3-056 as building on S8 and the scope; S1 F8 redaction), `WS05` (declared classes), `WS13` (approvals v2), DEC-21 (custody), DEC-26 (connector reads), DEC-27 (accounts and write authority); 057 REST kind also needs EP-9 slice 3 (egress).
- First slice 056 (M of L): editable preview for mail drafts and calendar events with per-tool policy levels; an edited payload must produce a new frozen descriptor and fingerprint (today the card offers only approve or reject; the descriptor is frozen and fingerprint-bound in the Do loop, per the card). Then the suggestion tool and resumable connect intent. 057 first slice: probe-and-test over `http_list_tools` returning tools and auth requirement without saving, and a tool picker; static bearer servers cannot register today because `validate_mcp_server` rejects any raw token, so the secret-reference step is a separate, custody-bound piece.
- Top risks: (1) writes: default always-ask; approval continuations are refused under ZDR; (2) 057 registration is refused under ZDR and secrets must never reach logs or model context; (3) plane boundary erosion: the REST executor and vault tempt a Model Plane-local secret store (audit 10.1 #1).
- Pull forward: the 057 probe and picker (no secrets, no egress) can run after `WS05`.
- Must be designed before starting: the secret-reference contract with integration-corev2; the editable-descriptor model with `WS13`.

### EP-11 Authored behaviours and plugins
- Items: F-t3-058 (one versioned lifecycle for skills, slash templates, presets, XL), F-t3-059 (plugin manifest loader with supply-chain pins, L).
- Owner: Model Plane (session-core `agent_skills`, capability-core `plugin_packages`/`skill_packages`), Control Plane (admin consent, kill list), Frontend Plane.
- Prerequisites: `WS19` (skill bundle store S6, `skill_file` S7, first-party bundles S9), `WS07` (F-t3-007 proposal path and learning ledger; `WS07` names Teach mode and F-t3-058/059 as consumers), `WS25` (extension allow-lists, D2: hook webhook deferred to F-t3-059 until an egress allow-list exists), `WS05`, DEC-30 (three tiers, promotion gate), DEC-40 (plugin supply chain).
- First slice 058 (M of XL): immutable versions with digests pinned on runs and a linter on every upsert path. Today `UpsertAgentSkill` overwrites in place and the learning loop persists candidates enabled with no proposal step (audit citations); a versioned `skill_packages` schema exists unused. Then draft/publish, Space and org libraries (a schema change), import, authored presets, `/learn` and Save-as-skill through the proposal path.
- 059 first slice: org-curated catalogue only. A loader that expands a plugin into child capabilities (skills, remote HTTP MCP servers, slash commands first), every source pinned to a commit SHA or archive digest, a kill list, every component listed at install; stdio and binary components are refused and reported. Catalogue and loader stay in capability-core; install is admin-only.
- Top risks: (1) supply chain: bundle scripts run only inside the sandbox bound to the low-risk `cap.command.sandbox` (audit 10.1 #10); (2) the gateway skill cache pulls once per org per TTL; user-scoped skills need a service-wide view; (3) a skill edit silently changing every matching run until versions land.
- Pull forward: version pinning is the part of 058 that `WS07` should not leave open: if the learning loop is enabled for real orgs before the staging gate, pin versions as part of that gate.
- Must be designed before starting: version and digest semantics across the three tiers (DEC-30); plugin manifest schema, kill-list distribution and the Control consent record.

### EP-12 Sharing, export and the public Agent API
- Items: F-t3-046 (sharing, replay, export and import, XL), F-t3-064 (public, versioned Agent API with origin-tagged runs, L).
- Owner: Model Plane (session-core snapshots, model-gateway routes, OpenAPI), Control Plane (grants in the resource taxonomy, API-key to bearer exchange, audit), Channel Plane docs-only (not a build target, DEC-40).
- Prerequisites: `WS17` (F-t3-012 for 046), `WS04` (F-t3-016), `WS03` (F-t3-002), `WS12` (F-t3-067 contract guardrails for 064), `WS15` (D7: F-t3-064 owns the run envelope origin), `WS20` S2 (run-level origin stamp), DEC-17 (closed run-origin enum), DEC-40 (share model: org-only default, viewer-bound connector calls, ZDR threads refuse sharing, replay and export; MCP Apps on the existing frame before any new origin).
- First slice 064: origin and parent id on every run (the `WS20` S2 stamp widened to all surfaces), then API-key to audience-bound bearer exchange in Control (auth-core's API-key plugin has no consumer today), then the public lifecycle surface over the existing model-gateway run routes (`http_routes.rs:132-236` as cited) with the OpenAPI spec and generated SDKs (`api/generate-sdk.sh`). First slice 046: owner-only whole-conversation export and a redacted replay view; shared snapshot links need a new classified type in Control's resource taxonomy (`resource_taxonomy.json` lists `document` as the only ownable type; Go embed, Rust mirror and lint script kept in parity) and are the part that waits on the ADR.
- Top risks: (1) ZDR threads have no durable transcript; sharing, replay and export must refuse them; (2) authority: unattended API runs need always-ask or pre-approved plans; API-started browser runs still need broker grants; grants belong in Control's taxonomy, never a Model Plane-local ACL; (3) imported conversations may carry prompt injections; the importer is a new untrusted-input boundary.
- Pull forward: origin tagging (DEC-17, `WS20` S2) is W2 work; 046's export slice needs only `WS17` S1-S3.
- Must be designed before starting: the share model ADR (DEC-40; no builder in W1-W2); the API-key exchange contract with Control; which surfaces become origin values.

### EP-13 Model record, providers, Gemini and BYO
- Items: F-t3-042 (model record v2 and role table, L), F-t3-041 (Gemini provider family on Vertex EU, L), F-t3-065 (declarative provider registry and bring-your-own connections, XL).
- Owner: Model Plane (inference-core capability record, session-core RoutingPolicy, model-gateway constants), Control Plane (pricing join via cost-core, entitlement), Integration Core (BYO custody), Frontend Plane (picker).
- Prerequisites: `WS10` (F-t3-023 entitlements: Control decides, Model enforces at admission; 042 depends on it), `WS11` (the per-model effort support table seeds the effort ladder; the Gemini thinking-level wire is a fourth `Wire` variant), `WS27` (pricing primitives, cost ledger), DEC-13 (entitlement ownership, ADR-0005), DEC-21 (BYO keys and custody), DEC-33 (ZDR attestation lifecycle), DEC-40 (Vertex EU residency and ZDR bar; BYO metering; neither has a builder in W1-W2).
- First slice 042 (M of L): extend `ListModels` (`inference.proto:481-525` as cited) with context window, max output and effort ladder; join price from cost-core by id and never copy across planes; picker search. Then the role dimension on RoutingPolicy replacing the hard-coded side-task constants, with a purpose tag on every call (title, follow-ups, judge calls pin `gpt-4o-mini` constants today, per the card).
- 041: attestation first, adapter second. The seeded Gemini row was soft-deleted because no adapter serves it; Gemini turns would be unmetered without pricing (audit 10.1 #5), so pricing precedes enabling. Adapter scope: tool-schema sanitiser, thought-signature replay, Gemini prompt profile (today prompt branching is a claude-prefix check).
- 065 first slice: providers as data (read-only admin overlay on `from_env` configuration) before any BYO connection. BYO connections are held by Integration Core and referenced by opaque connection id; a customer-billed route needs cost-core attribution and the served-by line must name the connection.
- Top risks: (1) an admin form must never be able to assert ZDR or residency: attestation is evidence-bound; (2) Vertex EU may not meet the residency and ZDR bar: the open question 10.2 Q13; (3) plane ownership: inference-core owns capability, cost-core pricing, session-core policy; join by id.
- Pull forward: the purpose tag on every inference call can ride `WS27` S2 (which already counts overhead calls) [suggestion, design check needed]. 042 slice 1 is the cheapest unblocker for EP-14's 040 and EP-5's image tool.
- Must be designed before starting: the record schema split by owner plane; the BYO custody and metering design (DEC-40 parked); the Vertex attestation path (commercial).

### EP-14 Run legibility, trajectory ledger and release gates
- Items: F-t3-034 (trajectory ledger with request capture and timing waterfall, XL), F-t3-048 (readable long runs, M), F-t3-040 (eval and replay release gates, L).
- Owner: Model Plane (model-gateway request snapshots at the call site, eval-lab), Frontend Plane (ledger view, transcript density).
- Prerequisites: `WS15` (F-t3-049 typed lifecycle events; 034 depends on it), `WS09` (transcript performance), `WS12` (F-t3-066 UI quality gates and F-t3-067 guardrails), `WS01` (release safety, F-t3-039), EP-13 slice 1 (F-t3-042 for 040), the ZDR resolver.
- First slice 040: make eval-lab a required stage when prompts or tool schemas change. Today `.github/workflows/eval-lab.yml` runs only the credential-free selftest on eval-lab changes; 14 YAML cases exist and no comparison or promotion code exists despite the README claim (audit citations). Replay of recorded sessions in CI follows; the RoutingPolicy edit becomes a candidate, evaluate, promote flow. 034 first slice: request snapshots (exact system prompt, tool catalogue, parameters) written at the model call site, owner-only; the ledger view and waterfall after. 048 first slice (M): per-user transcript density levels and "Worked for X" folding of settled turns, then the docked "Step N of M" strip and tool narration; nothing in the SPA reads the persisted `compactMode` today.
- Top risks: (1) recorded sessions must be synthetic or scrubbed and never ZDR turns; request capture is a new content store (audit 10.1 #2); (2) which platform prompt text is shown to the user is a product decision; access owner-only, no ACL widening; (3) experiment ids and user-path budgets inside 040 are separate S/M items: do not let them stretch the epic.
- Pull forward: 048 and the 040 CI stage are FE/CI-only and can start as soon as `WS09` and `WS12` are green; 048 is independent of the other two. Any recap model call for narration must not run on ZDR turns.
- Must be designed before starting: the request-snapshot schema and retention; the eval case format and the gate thresholds.

---

## 5.3 Cross-epic dependencies worth tracking

| From | To | Why |
|---|---|---|
| WS22 (F-t3-009) | EP-1, EP-9 (043), EP-4 (via 087 later) | Sandbox computer is the trunk of the computer track; 043/084/085/087 all wait on it |
| WS03 (F-t3-002) | EP-1 (026), EP-2 (036), EP-7, EP-12 (064) | Durable loop is the trunk; OD8 renewable bearer is a hidden prerequisite for EP-7 and 036 |
| WS05 (F-t3-003) | EP-4 (045), EP-10, EP-11 (059) | No MCP execution until a durable per-tool binding exists; never synthesised from a model-supplied name |
| WS17 (F-t3-012) | EP-3, EP-12 (046) | Index in session-core; message tree follows the index |
| EP-13 (042) | EP-14 (040), EP-5 (051), W5 072/073/095 | Model record before provider breadth |
| EP-9 (043 egress) | EP-10 (057 REST kind), W5 084 | Proxy before custom REST connectors and per-Space images |
| EP-8 (060) | EP-6 phase 3 | Scoped memory before persistent named agents |
| EP-5 (WS19 verifier) | W5 090/091 | Office/attachments lane before time-based media and dashboards |

[RECONCILE] items collected in this section: EP-3 two versioned-document stores (overlap-space row 13); EP-6 definition store vs `WS25` D1 policy store (DEC-14); EP-7 Watch design written twice (overlap-space row 15); EP-9 secret custody (DEC-21) and policy store (DEC-14, DEC-19). No other contradictions among the designs were found while compiling the epics.

---

## 5.4 W5 nice-to-have backlog

Rule: schedule on demand, or when a dependency makes the item cheap. None is designed. Prerequisite column names the single gating item; other audit dependencies are in the notes.

| ID | Title | Effort | Single prerequisite | Schedule on demand: note |
|---|---|---|---|---|
| F-t3-069 | Rewind with undo preview, side chat, spin-off tasks | M | F-t3-012 (`WS17` S8/S9 message tree) | After versions and confirmed rewind ship; side chat also needs EP-3 |
| F-t3-070 | Tool efficiency at scale (deferred index, code mode) | XL | F-t3-003 (`WS05`) | Only when the tool catalogue measurably costs tokens; `WS25` S4/S5 handles already trim output; code mode also needs F-t3-009 |
| F-t3-071 | Coordinator-mode orchestration on Temporal | M | F-t3-002 (`WS03`) | After EP-6 phase 2; reuse the shared LoopGuard (`WS25`) |
| F-t3-072 | Resilient routing | M | F-t3-042 (EP-13) | Subscription fallback clashes with ownership-matrix rule 17: needs explicit reversal; also needs F-t3-073 |
| F-t3-073 | Per-step routing decision and model continuity | L | F-t3-042 (EP-13) | After role table exists |
| F-t3-074 | Editable permission rules, CEL policy, MCP tool gate | L | F-t3-006 (`WS13`) | Also needs F-t3-032 (`WS25`); only if admins ask for rule editing beyond modes and hooks |
| F-t3-075 | Privacy by construction and a closed telemetry contract | L | F-t3-067 (`WS12`) | Pairs with EP-9 egress catalogue (068); also needs F-t3-004 (`WS06`) |
| F-t3-076 | Changed-files card and review gate before promotion | L | F-t3-030 (`WS24` S4) | Also needs EP-3 (F-t3-031); DEC-24 (promote) decides where edits become visible |
| F-t3-077 | One action-registry-driven command surface | M | F-t3-025 (`WS24` S5/S6) | Also needs `WS17` S2 (history action contract) |
| F-t3-078 | Feedback surfaces and limit notices | S | F-t3-066 (`WS12`) | Cheap once UI gates exist; limit notices pair with `WS27` S10 |
| F-t3-079 | Typed feature-flag and settings registry | M | F-t3-039 (`WS01`) | Absorbs the flags-and-config ledger; Control Plane owns |
| F-t3-080 | Library of generated and uploaded files | M | F-t3-031 (EP-3) | Data Plane and Control Plane; share model constraints (DEC-40) apply |
| F-t3-081 | Teach mode | XL | F-t3-026 (EP-1) | Also needs F-t3-008 (`WS21`) and F-t3-007 (`WS07` proposal contract); Quarry owns procedure compile/replay (overlap-quarry) |
| F-t3-082 | Upgrade voice input | L | none | Standalone; schedule when a customer asks |
| F-t3-083 | Run admission controls | L | F-t3-028 (`WS27`) | Also needs F-t3-002 (`WS03`); Control org-core is the one quota source |
| F-t3-084 | Per-Space environment specs and prebuilt images | L | F-t3-009 (`WS22`) | Demand evidence is thin (audit 10.3); needs EP-9 egress (043); do not schedule without a concrete user need |
| F-t3-085 | Sandbox computer extensions | L | F-t3-009 (`WS22`) | Demand evidence thin (audit 10.3); also needs F-t3-008 |
| F-t3-086 | Review as a run | M | F-t3-030 (`WS24`) | On demand |
| F-t3-087 | Multi-file app build and preview | L | F-t3-030 (`WS24`) | Also needs F-t3-009 and F-t3-045 (EP-4) |
| F-t3-088 | Code edit quality on demand | S | F-t3-030 (`WS24`) | Demand evidence thin (audit 10.3); S when the edit engine exists |
| F-t3-089 | Use Verevon from other agents and work tools | XL | F-t3-064 (EP-12) | Channel Plane is docs-only (CLAUDE.md, DEC-40): do not build until a Channel Plane ADR; also needs F-t3-025 |
| F-t3-090 | Time-based media on demand | XL | F-t3-017 (`WS08`) | Also needs F-t3-041 (EP-13); Data Plane and Frontend |
| F-t3-091 | Data-connected dashboards and label-aware document policy | L | F-t3-017 (`WS08`) | Dashboard half rests on one of twelve sources and documentation only (audit 10.3) |
| F-t3-092 | Model Council | M | F-t3-028 (`WS27`) | Multiplies spend; schedule only with the meter in place |
| F-t3-093 | Grow the corpus from research | L | F-t3-054 (EP-2) | Data Plane; ZDR and source-trace rules apply |
| F-t3-094 | Self-maintained team knowledge wiki | XL | F-t3-007 (`WS07`) | Data Plane; consumes the learning proposal gate |
| F-t3-095 | Second-order latency and cache levers | L | F-t3-022 (`WS02`) | Also needs F-t3-042; the Responses-continuation assumption is inference [INF] |
| F-t3-096 | Operability long tail | M | F-t3-004 (`WS06`) | Also needs F-t3-075 |
| F-t3-097 | Tighten-only runtime monitors | XL | ADR on an LLM on the permission path (DEC-40) | Standing rule and ownership-matrix rule 12 forbid it by default; only shadow mode via the hook engine, never granting, fails closed to a human card |
| F-t3-098 | Reader and appearance polish | M | F-t3-021 (`WS09`) | Also needs F-t3-066 (`WS12`); can pair with EP-14's 048 |

Pull-forward candidates from W5: F-t3-078 (S, after `WS12`), F-t3-088 (S, after `WS24`), F-t3-079 (M, after `WS01`) are the cheapest. F-t3-082 (voice) has no prerequisite at all.

---

## 5.5 What must exist before any W3/W4 epic starts

- A named designer per epic and a file-level design in the `WSnn` format (scope, slices with effort, verified paths, ZDR clause, proof). This section is the input to that, not a replacement.
- The decisions named per epic closed or explicitly deferred: DEC-14, DEC-19, DEC-21, DEC-22 (blocking EP-1/9/10/11), DEC-28/29 (EP-3, EP-8), DEC-40 items (EP-4, EP-12, EP-13).
- The W1 trunk items (`WS03`, `WS05`, `WS17`, `WS22`) verified live, not only designed: the audit lists several "built" states it could not prove from code (audit 10.3): outbox fix deployment, signed release candidate, Redis in every deployment.
