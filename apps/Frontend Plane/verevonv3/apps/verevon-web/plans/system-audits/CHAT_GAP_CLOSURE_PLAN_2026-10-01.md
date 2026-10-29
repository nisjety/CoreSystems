# Plan to close the chat parity gaps (2026-10-01)

Built from the [parity audit](CHAT_PARITY_AUDIT_2026-10-01.md) by workflow `chat-gap-closure-plan`: ten workstream designers (A to J) read the code and the audit rows and proposed 139 tasks, then a sequencer merged them into seven waves, removed duplicates, found file collisions and consolidated the owner decisions. The full task specs (files, approach, tests) are in `chat-gap-closure-plan-2026-10-01.json` next to this file. Workstream J (trust and owner-only items) first returned empty; it was designed again later and is in section 3, sequenced separately.

## 0. Status (updated 2026-10-02)

**Proven live on the ChatGPT Terra route** (see [the evidence note](A6_DO_MODE_TERRA_PROOF_2026-10-01.md)): the Do loop, `plan.propose`, `knowledge_search`, `web_search` and a three-way wide-research fan-out with durable child runs and lineage; every round on the subscription route, no cost-core entries. The proof found six defects that unit tests had missed (an invalid tool schema, two unattested capabilities, missing Quarry scopes, child runs never registering, and children losing their findings), all fixed. Also proven live: redeploy-safe turns (I3), the planner action JSON on Terra (E3: the strict schema must be used, the plain one is rejected), and the per-answer processing line (J14).

**Built, reviewed and deployed:** A0 to A5, A7, B8 (off by default), B10, D2, D9, D12, E3, E7, F1 to F4, G1, H1, H2, H5, I3, J1, J2, J3 (dark), J9, J14, and the compliance evidence J10 to J13 and G12 (documents; the proposed /trust edits are a diff and are not applied). Deployed to the Control, Model, Ingestion and Frontend planes on 2026-10-01 and 2026-10-02.

**Not proven live, needs the owner:** the connector reads (Outlook, Gmail, OneDrive, SharePoint: G1 and F2 to F4 need a real reconnect, then the operator steps in each report), the live Office-output proof for H5 (`H5_LIVE=1` and the runbook), the editorial and counsel review of J13, and G12 (publisher verification is an owner action).

**Owner decisions still open:** whether Do on Terra stays on by default (it is on in the dev stack through the launcher environment only), ZDR child registration, the paid contextual-retrieval and graph-extraction paths, applying the /trust edits, and the items listed in the evidence note.

**Next:** Wave 1 (the Office and connector rows), the Quarry search-quality fix (its own retrieval and rerank principals get 403), and a short Q16 re-run before any release because the build has changed since the final pass.

## 1. The plan in one paragraph

One plan, 7 waves, covering workstreams A to I. Workstream J returned an empty placeholder with no tasks or decisions, so its trust and data-protection items are not sequenced here; re-run the J design before Wave 1 ends.

Duplicates merged so nothing is built twice:

- A3 and C2 (agentic stream follow) are one task owned by A3.

- A7 folds into E4, and A8 folds into D10.

- G2 folds into H1 and H10, and F14 folds into G3.

- I14 folds into H9.

- G4's calendar view and the Microsoft calendar bundle fold into F3 and F5, and F5's reconnect union folds into G1.

- Three wall-clock and round-budget mechanisms (A4, C3, C4, B6, C5) collapse into one resolve_run_budget function plus one credential-expiry guard.

Wave outline:

- Wave 0 builds Do mode on gpt-5.6-terra dark behind one kill switch (A1) and closes the paid learning-review leak (A0). It also settles the compile, identity, Postgres and digest facts that gate later work. It needs no Terra quota.

- Wave 1 wires the built-but-unwired paths: plan grant plus persona, org and hook context in agentic runs; child-run registration; the scheduled-lane substrate; hosted-browser grants and a model planner on the user's route; working mail, calendar and file executor ops. It also includes the Office-first items that need no Do mode (H1, H2, H5).

- Wave 2 is the first Terra proof window, after the Codex reset on 2026-10-03 17:45 UTC, plus agent-path completion.

- Wave 3 closes B to F end to end.

- Waves 4 and 5 hold H (Office and data outputs), G (connector rows; reads in Wave 4, writes in Wave 5) and I (table stakes).

- Wave 6 is deferred.

Two flagged exceptions to the rule that G and I come after B to F:

- G1 is pulled into Wave 1 because it is a consent-correctness fix that F5 needs.

- I3 is pulled into Wave 1 because it stops launcher redeploys from killing in-flight Terra proof turns.

Three cross-cutting facts shape the whole plan:

- The 300 s delegated bearer bounds every A, B, C and E run.

- B's executing run cannot be proven on Terra under A's read-only rule unless effects are allowed on the proof stack with the ask posture.

- Six proto files and about ten hot source files are shared, so the ownership and ordering rules in file_conflicts are mandatory.

Convention: parallel_groups are listed in phase order. Tasks inside one group may touch the same hot file only in the disjoint functions named in file_conflicts. Additive-seam files take small re-read-before-edit hunks only. These are docker-compose.yml, gateway http_routes.rs, use-chat-controller.ts, ChatMessages.tsx, ChatPage.tsx, chat-types.ts and chat-normalizers.ts. Agents never commit, stash or revert.

## 2. Standing rules the plan keeps

- Live tests run only on the ChatGPT Terra subscription route, never on Verevon Balance or another paid provider. That is why workstream A (Do mode on the Terra route) comes first: plan mode, wide research and the agent's connector tools cannot be proven live without it.
- Owner rulings of 2026-09-28 stand: scheduled runs are draft-only, personal connections are owner-only, Chrome control means the hosted governed browser, mail-to-task is Postmark self-forward.
- Review changes need an A/B on the saved matrix. No commits by the agents. Images are built through the launcher; the frontend container is restarted only after a `git status` check because another session has uncommitted frontend edits.
- **Correction to the waves below:** the sequencer used the owner-memory date 2026-10-03 17:45 UTC for the Codex usage reset. The owner already reset the limit on 2026-09-30 and the final Q16 pass ran on it, so the first Terra proof window (Wave 2) is open now. It is still limited by quota, so spend it in the order of the proof ladder.
- Everything in waves 0 and 1 is built dark (switched off by default) and proven with unit tests, end-to-end tests and free negative probes before it is switched on.

## 3. Waves

### Wave 0 - Unblock proof, plug the paid leak, settle gating facts

**Goal:** Do mode (RunAgent) runs on gpt-5.6-terra via openai-codex-subscription behind one kill switch, built dark and proven with unit tests, e2e tests and free negative probes. The hidden paid learning-review call is closed. integration-corev2 owner-only tests compile, the plan-gate SQL runs on real Postgres, the template digest matches between BFF and Go, and the Terra planner schema probe is written. No Terra quota is needed. Groups are in phase order: A2 and A5 start after A1 lands, A3 and A4 after A2, and A6 last.

**Build order (tasks in the same group can be built at the same time):** 1. A0, A1, B10, D2, E3, F1, G12; 2. A2, A5; 3. A3, A4; 4. A6

| Task | What | Size | Depends on | Owner decision |
|---|---|---|---|---|
| A0 | Stop paid-provider leakage before any Terra live run (background learning review) | S |  |  |
| A1 | Carry the subscription route through RunAgent: proto, gateway, execution-core, kill switch | M |  |  |
| A2 | Typed subscription failures, run-level refusal latch, and a content-free usage log | M | A1 |  |
| A3 | Agentic stream: follow the run concurrently, drop the 25 s idle floor, route-aware deadline | S |  |  |
| A4 | Server-side run budgets for subscription Do runs (rounds, subagents, wall clock, concurrency) | S | A1 |  |
| A5 | Frontend: let Do work on a subscription model when the server says it can | S | A1 |  |
| A6 | Live proof harness and runbook on the Terra route (plus the free negative proofs) | M | A0, A1, A2, A3, A4, A5 |  |
| B10 | Run the plan-gate SQL against real Postgres (baseline and regression net) | S |  |  |
| D2 | Fix template digest parity between the BFF and Go before adding fields | S |  |  |
| E3 | Spike: can Terra return the planner's action JSON? Decide the schema variant | S |  |  |
| F1 | Make delegated identity verifiable and the owner-only tests runnable | S |  |  |
| G12 | Publisher verification and Entra/Google app registration runbook (owner action plus a docs file) | S |  | yes |

**Exit criteria:** A1 and the free probes: - With the switch off, a Do request on the subscription route gets the typed 400 agentic_subscription_disabled and no run is created (A6 P1). - With the switch on in the proof stack, a bogus subscription_connection_id ends with a typed subscription error. inference-core logs show no non-subscription provider for that request (A6 P2). - Every RunAgent round of parent and child carries provider, connection and model. EXECUTION_AGENT_MODEL is ignored on a subscription run. A provider_used mismatch ends the run. A0: capability-core logs show the learning-review consumer disabled on the proof stack, and a log check records whether it fired before. A2 to A5: unit and e2e green. - The paused-time test shows no 25 s tail after a finished run. - Images build through the launcher model-plane-only copy (LAST_STACK_INDEX=2). - cargo check of Data Plane v2 is clean after the execution.proto change, and Go wire-parity is green. Other items: - B10: plan-gate tests pass on a scratch Postgres database. - D2: the golden digest vector passes in Rust and Go, and a goal containing '&' creates. - F1: integration-corev2 internal/api compiles, the owner-only tests run, and the exchange probe result is recorded (400/401 means the grant is active, 403 means it is not). - E3: the ignored test is written; its 3 to 5 Terra calls run in the first proof window. - G12: the owner has started publisher verification.

**Needs from the owner:** OD1 (snapshot shared WIP), OD2 (kill-switch default and effects posture), OD4 (learning review), OD5 (proof window), OD20 (frontend rebuild coordination). The owner starts G12 (publisher verification, external lead time) and decides OD16.

### Wave 1 - Wire the built-but-unwired paths (dark, unit and e2e proven)

**Goal:** Fix what exists but is not end to end: - Plan grant, persona, org and hook context in agentic runs: B1, B3, B4, B8. - Child-run registration, durable lineage cards, research-only child tools: C1, C7, C8. - Scheduled-lane substrate (policy, run profile, result persistence, truthful status, 'Varsle meg', read-boundary fix): D1, D3, D5, D6, D9, D12. - Hosted-browser grants, broker policy, planner on the user's route, risk classifier: E1, E2, E4, E6, E7. - Mail, calendar and file executor ops plus an additive consent path: F2, F3, F4, F5, G1. - Office-first items needing no Do mode: H1, H2, H5. - Redeploy-safe turns: I3. Nothing is switched on by default. Groups are in phase order.

**Build order (tasks in the same group can be built at the same time):** 1. B1, C1, B8, D1, D9, E1, E2, E7, E6, F2, G1, H1, H5, C7; 2. C8, B4, D5, F3, F4, F5, H2, I3; 3. B3, D3, E4; 4. D6, D12

| Task | What | Size | Depends on | Owner decision |
|---|---|---|---|---|
| B1 | Durable per-plan grant and an explicit execute_plan_id intake in the gateway | M |  |  |
| B3 | Pass persona, org and Space instructions (and identity) into agentic runs | M |  |  |
| B4 | Feed org hook_context into agentic runs | S |  |  |
| B8 | Post-mutation verification inside the loop (provider writes and shipment booking) | M |  |  |
| C1 | Make delegated child-run registration real (cards, lineage, durable child answers) | S |  |  |
| C7 | Cards that survive reload, show queue and limits, and carry each item's answer | M |  |  |
| C8 | Research-only toolset for fan-out items | S |  |  |
| D1 | Activate the existing scheduled lane: policy grants, credentials, contract test, readiness check | M |  |  |
| D3 | Run profile in the Control-committed template, bound through Session Core | M | D2 |  |
| D5 | Session Core persists the result into the scheduled thread and terminalizes the run | M |  |  |
| D6 | Runs endpoint, cron_fires thread link and truthful status | M |  |  |
| D9 | 'Varsle meg' control in the waiting state and the live run panel | S |  |  |
| D12 | Close the org-wide read of system-owned scheduled threads and verify the personal-Space read live | S |  |  |
| E1 | BFF mints and forwards the browser-broker bearer; stop sending the fake session:{id} grant | S | E2 (deploy order only) |  |
| E2 | Real grant issuer in model-gateway plus a broker-owned domain policy, grant lifecycle and run clamps | M |  | yes |
| E4 | Planner on, routed to the user's subscription, fails closed, and 'done' carries the answer | M | E3 for the strict-schema choice only; the rest can start now | yes |
| E6 | Live record for hosted-browser tabs, DevTools, WS frame/action transport and replay | S |  |  |
| E7 | Risk classifier hardening: Norwegian markers, password-field detection, regression table | S |  |  |
| F2 | integration-corev2 mail: real search, metadata, folders, paging and attachment metadata (Outlook and Gmail) | M |  |  |
| F3 | integration-corev2 calendar read: windowed, searchable, trimmed, in local time (Outlook and Google) | M |  |  |
| F4 | integration-corev2 files and SharePoint: libraries, folders, paging, Drive shared drives, and the missing Microsoft dri… | M |  |  |
| F5 | Calendar connect path: grant Calendars.Read for Microsoft without narrowing existing connections | S | F3 (for the live check only) |  |
| G1 | Make scope upgrades additive and truthful (precondition for every new bundle) | S |  |  |
| H1 | Extractors that read what Excel and PowerPoint actually store (formulas, dates, notes, tables, charts, zip guard) | M |  |  |
| H2 | Native chart renderer in chat: chart fence, table-to-chart, copy and CSV, steering notice | M |  |  |
| H5 | code_interpreter on Terra: advertise the whole library set, probe imports, first live proof of docx, pdf, pptx, xlsx an… | S |  |  |
| I3 | Redeploy-safe turns: graceful drain, stop grace period and REDIS_URL on model-gateway (1 to 2 days) | S |  |  |

**Exit criteria:** - Unit and e2e suites pass for every task. - Images are built only through the launcher. Model plane uses LAST_STACK_INDEX=2; BFF, imports and ingestion use their own stages. The frontend container is restarted only after a git status check; otherwise verify on :5199. - Data Plane v2 cargo check is clean after each proto batch. Model-free live proofs: - B1: the plan grant reads back after a model-gateway restart. - E1 and E2: a hosted-browser run with a broker-issued grant starts and completes on the deterministic planner, with no 403 and no model call. - C1: the probe shows user-bearer registration accepted and records whether the service token was refused. - D1: the readiness script reports every binding present. The owner applies the policy. - F2 to F4: httptest suites green and integration-corev2 builds. - H1: real Excel and PowerPoint fixtures extract correctly (formulas, dates, notes). - E6: a dated record of tabs, DevTools and replay. - I3: model-gateway drains in-flight streams on SIGTERM.

**Needs from the owner:** OD1 (snapshot before C7, D1 and C8 start), OD6 (plan-gate defaults), OD9 (D1 apply timing), OD13 (E2 and E4 domain scope and planner route), OD10 (F-chain defaults).

### Wave 2 - First Terra proof window and agent-path completion

**Goal:** After the Codex window resets (owner memory: 2026-10-03 17:45 UTC; confirm with one canary turn), run the Do-on-Terra proofs (A6 P3 to P5) that unblock B, C and F. Also run the queued W0 and W1 live proofs from the proof ladder: E3, H5, H1, H2. Then finish the agent paths: - Plan approval starts the executing run, with step recitation, durable receipts and a round budget. - Wide-research budgets, breaker and gateway clamps. - The agent mail, file and calendar read tool with untrusted-content handling. - The scheduled agent step, notifications and results UI. - Browser run live proof and answer-in-thread. - The picker chain proven on Terra. Code work continues before the reset; proofs are queued.

**Build order (tasks in the same group can be built at the same time):** 1. C3, B2, D4, D7, E8, F9, F6; 2. C4, C5, B5, F8, D8; 3. B6, B9, F11a, F10, E5

| Task | What | Size | Depends on | Owner decision |
|---|---|---|---|---|
| B2 | Approving a plan starts the executing run from the web app | M | B1 |  |
| B5 | Recite the approved plan checklist into the context tail; the model marks steps; progress is durable | L | B1, B10 |  |
| B6 | Give executing runs a round budget that fits the plan | S | B1 |  |
| B9 | Bind the approved plan to the run's receipts (ApprovalProof.plan_id and run provenance) | M | B1, B10 |  |
| C3 | Child round budget separate from the parent's rounds, with request fields | M |  | yes |
| C4 | Guards that make the limit honest: credential deadline, provider breaker, run and org concurrency | M | C3 |  |
| C5 | Request fields, policy and org quota at the gateway (client asks, server clamps) | M | C3 |  |
| D4 | Execution-core scheduled agent step: bounded read-only tool loop, unattended draft-only posture | L | D3 |  |
| D7 | Notifications: run-watch context and deep link, auto-watch for scheduled fires, failure notice, navbar link | M | D6 |  |
| D8 | Frontend: 'Se resultater', results view, deep link, 'Kjor na', 'Fortsett i chat' | M | D6, D5 |  |
| E5 | Live proof on Terra of the agent run from the Home surface, with HITL, plus doc updates | M | E1, E2, E4 |  |
| E8 | Deliver the run's answer into the chat thread | S | E4 | yes |
| F6 | execution-core: a read-only provider tool that does not stall on approval, with real catalog, paging and size-aware res… | L | F2, F3, F4 (develop against fixtures of the additive shapes first, then refresh) | yes |
| F8 | Treat connector content as untrusted and personal: trust class, classification, durable redaction, ZDR refusal | M | F6 (same seams in agent.rs and mod.rs; land after it or use one implementer) |  |
| F9 | Composer picker: readable Gmail rows, search that works, folders and paging, specific calendar message | M | F2, F3, F4, F5 |  |
| F10 | Live proof on the Terra route with no Do mode: the connector chain through the composer picker | M | F2, F3, F4, F5, F9 |  |
| F11a | Shared-inbox tools: HTTP-level tests and untrusted framing for mail bodies | S |  |  |

**Exit criteria:** A6 P3 to P5 on Terra: - Tool steps and an answer on a Do turn. - A plan_proposed frame. - A 3-item fan-out with attach and stop events. - Dated evidence of zero requests to any non-subscription provider and no cost ledger rows. B2, B5 and B9: - An approved plan starts the executing run from the web app. - Steps are recited and ticked. - The proof bundle names the plan id and rung. - Live proof of the executing run needs effects allowed on the proof stack with the ask posture (OD2). Use a read-only 2 to 3 step plan. C3 to C5: - Fan-out items get a real round budget. - The org clamp shows 3 of 6 items run. - The breaker is proven by mocked tests only. F6 and F8: - read_provider_action never pauses for approval, which settles the capability-gate finding. - Session-core step records hold no mail or file text. Other items: - F10: the picker chain on Terra passes the design's assertions. - E5: the Terra planner takes at least two non-observe actions. The login gate is approved and denied. The honest-failure path shows no silent Observe loop. - D4, D7 and D8: unit and Postgres proven. - If Terra reports a cap, stop and record it. Never switch model.

**Needs from the owner:** OD2 (effects on proof stack), OD3 (quota numbers), OD5 (proof window), OD7 (child budget design), OD10 (autonomous own-data reads), OD12 (proof accounts connected by the owner), OD13 (hosted-browser rules).

### Wave 3 - Close B to F end to end

**Goal:** Finish the large items and record the proofs: - Plan continuation after an approval pause. - Wide-research entry point and cards that survive reload. - Longer run credentials, only if measurement demands it. - Unattended owner identity for scheduled runs, behind a security gate, then the scheduled-run proof on Terra. - Browser bridge from /chat with live view and takeover. - Office-file text for the agent. - The shared-inbox ownership model. - Dated evidence rows in the gap plan. G and I work may start only after Wave 2 exit is met and Wave 3 group 1 is merged.

**Build order (tasks in the same group can be built at the same time):** 1. B7, C6, E9, D10, F11b, G16; 2. C9, D11, E10, F7; 3. B11, C10, D13, F13

| Task | What | Size | Depends on | Owner decision |
|---|---|---|---|---|
| B7 | Continue the plan after an approval-gated action | L | B1, B2, B5 | yes |
| B11 | Record the live proof on the Terra route once Do mode runs there | S | B1, B2, B3, B4, B5, B6, B9, A |  |
| C6 | User-facing entry point: a 'Bred forskning' control in Do mode | M | C5 |  |
| C9 | Credential lifetime for long wide-research turns (security-reviewed) | M | C4 | yes |
| C10 | Terra live proof and measurement record | S | A, C1, C3, C5, C6, C7 |  |
| D10 | Unattended owner identity for the subscription route (decision-bound token exchange, active-connection resolution) | L | D1, D4 | yes |
| D11 | Guards and surfaces: cadence floor, quotas, auto-pause, ZDR block, 'Kjor na', re-enable /schedule on Terra | M | D6 | yes |
| D13 | Terra live proof script and evidence entry | S | D1, D3, D4, D5, D6, D7, D8, D9, D10, D11, D12 |  |
| E9 | Chat bridge: 'Open in browser' from a message and /browse, watched in the live panel | M | E1, E2, E4, E8 for the final message |  |
| E10 | Live view and human takeover of the agent's browser from the chat panel | L | E9, E4 | yes |
| F7 | File and attachment text for the agent: extraction through imports-core, paged | M | F4, F6 | yes |
| F11b | Ownership rule for mail already synced into the org-wide inbox (connection visibility) | L | F1 | yes |
| F13 | Delegated agent read: live proof in a Do run | S | A, F1, F6, F7, F8 |  |
| G16 | Compliance and record updates for connector content, writes and contacts | S |  | yes |

**Exit criteria:** - B7: a manual 'Fortsett planen' continues a plan after an approval pause. - C6 and C7: the 'Bred forskning' toggle sends a clamped request and the cards survive a reload. - C9: built only if OD8 says yes and the security review passes. - C10: a 6-item Terra run proves child registration, lineage, reload and no budget-Exhausted. It records p50 item time to set the OD3 numbers. - D10: passes adversarial review before activation. - D13: a scheduled run on Terra proves the notification, the result drawer, the failure cases and zero Balance spend. - E9 and E10: from a chat answer, open the hosted browser, watch steps, approve a gate, take over and hand back. - F13: the eight checks pass, including delegated reads without approval pauses. - F11b: enforces owner-only for personal connections in the shared inbox. - G16: compliance drafts recorded as pending counsel. - B11: the executing-run proof is recorded. - Gap-plan rows are updated only with observed evidence.

**Needs from the owner:** OD5 (D10 go/no-go and security review), OD8 (credential lifetime), OD11 (shared inbox), OD13, OD15 (policy batch applied by the owner through the Control launcher), OD12.

### Wave 4 - Office and data outputs, read-side connector rows, chat-core quick wins

**Goal:** Owner priority is Office documents first. Ship: - Direct PDF, charts in Office exports, staged data in the sandbox, local OCR, and xlsx/pptx in knowledge. - The read-side connector rows: Excel and Sheets range read and attach, Office-file sync built dark. - Hosted-browser prod parity. - Small chat-core table stakes that also fix silent wrong behaviour: shortcuts, idempotency key, streaming render, citation binding, skill picks on Terra, supersede and branch, search, templates, usage meter, and the reasoning spike. Gate: Wave 2 exit met and Wave 3 group 1 merged.

**Build order (tasks in the same group can be built at the same time):** 1. H3, H4, H9, H10, G4, I1, I2, I5, I22, E13; 2. H6, I9, G5, E11, I7, G3, I15; 3. I6, I8, I12, I13, I16

| Task | What | Size | Depends on | Owner decision |
|---|---|---|---|---|
| H3 | Direct PDF: a real .pdf file from the browser, no print dialog | L |  | yes |
| H4 | Native charts inside Word, Excel and PowerPoint exports | M | H2 |  |
| H6 | Stage attached documents into the sandbox so the model can analyse data exactly | M |  |  |
| H9 | Scanned and image-only PDFs: local OCR fallback in imports-core | M |  | yes |
| H10 | Allow .xlsx and .pptx in knowledge imports | S | H1 |  |
| G3 | Office-format extraction for synced SharePoint/OneDrive files in finspo-core (build dark) | M | G2 (share fixtures and extraction rules) | yes |
| G4 | Server-side read surface: calendar view, Graph workbook reads, Google Sheets reads, Microsoft calendar connect bundle | M | G1 |  |
| G5 | Picker: live range attach for Excel/Sheets, Outlook calendar connect prompt, error copy | M | G4 |  |
| E11 | Prod parity: grants for manual sessions and a working Quarry validator | M | E2 |  |
| E13 | Window share step 0: test and harden the existing one-shot capture | S |  |  |
| I1 | Keyboard-shortcut sheet opened by ? and a menu entry (about 1 day) | S |  |  |
| I2 | Send an idempotency key with every user send and handle duplicate_in_flight (about 1 day) | S |  |  |
| I5 | Block-level streaming render and frame-batched deltas, with a measurement (2 to 3 days) | S |  |  |
| I6 | Citation chips bind by number, one numbering across producers, and a real live proof (2 to 3 days) | S |  |  |
| I7 | Allow explicit skill picks on the Terra route (about 1 day) | S |  |  |
| I8 | Server-owned branch conversation (about 1 week) | M |  |  |
| I9 | Supersede primitive so regenerate, edit and continue stop polluting model history (1 to 2 weeks) | M |  |  |
| I12 | Full-text search over past chats plus a recall_conversations tool (about 1 week) | M |  |  |
| I13 | User-authored slash command templates with $ARGUMENTS, then a narrowing tool allowlist (1 week, phase 2 about 2 days) | M |  |  |
| I15 | Visible reasoning on the Terra route: spike, then three-hop plumbing (spike 1 day, build about 1 week) | M |  |  |
| I16 | Live usage meter during a run, then a subscription quota gauge (about 1 week) | M |  |  |
| I22 | Record two non-build rulings: no duplex voice, no presence in plain chat (docs only), plus the owner real-microphone ch… | S |  | yes |

**Exit criteria:** - H3: a real PDF downloads from a Terra answer with no print dialog and no window.print call. - H4: exports open in real Word, Excel and PowerPoint with no repair prompt. - H6: pandas totals match a Python reference. - H9: local OCR finishes inside the 18 s BFF budget. - H10: xlsx/pptx import, then a cited answer. - G4 and G5: live range attach on a real workbook and Sheet through Terra. - G3: built dark with its flag off. - E11: proven on a throwaway quarry-edge. This is required before any ENVIRONMENT=prod deploy of the hosted browser. - I2: a duplicate send returns duplicate_in_flight. - I5: the mount counter shows no per-tick remounts and long-task numbers are recorded. - I6: citation chips bind by number. - I7: an explicit skill pick works on Terra. - I9: regenerate, edit and continue no longer pollute model history (check the context inspector). The four-journey regression passes. - I8: a branch carries server-side history. - I12: search finds body text. - I13: a template expands. - I15: the spike answers whether Codex emits reasoning summaries for Terra. - I16: the usage counter equals the final usage. - I22: the two rulings are recorded.

**Needs from the owner:** OD18 (fonts, OCR, caps, truncation, deck theme), OD19 (rulings for I22 and I-items), OD17 (G3 default off), OD16.

### Wave 5 - Writes, designed deck path, deeper chat core

**Goal:** G rows that write start only after reads are proven. Order of work: - Always-ask in front of every provider write. - The human-click apply path. - Calendar and sheet proposal cards. - Save-to-cloud. - Edit-in-place for Office files. - The designed deck path. - Heavier chat-core items: restart orphan settling, server-side versions, rewind, canvas phase 1.

**Build order (tasks in the same group can be built at the same time):** 1. G7, G6, H8, I4, I10, I17; 2. G8, G10, I11; 3. G9, G11; 4. G13, G14, G15

| Task | What | Size | Depends on | Owner decision |
|---|---|---|---|---|
| G6 | Edit an existing xlsx/pptx/docx in place: staged binary, verevon_office helper, fidelity check, continue-editing | L | Terra proof that code_interpreter runs and delivers files (other workstream or this task's first live step) |  |
| G7 | Write executor ops in integration-corev2 (calendar, drive upload/replace, Graph workbook range, Sheets values), built d… | L | G1 |  |
| G8 | execution-core: integration:write grant, always-ask, 'approved' mode, effect summaries, catalog, read-back verification | M | G7, launcher run from the Control Plane so the policy takes effect (as for delegated identity, 04-implementation-and-deployment-status.md) | yes |
| G9 | Human-click apply path: BFF route, model-gateway route, ExecuteStep with approved mode | M | G7, G8 |  |
| G10 | Consent: incremental write bundles, per-bundle consent screen, reconnect that keeps grants | M | G1, G7, G11 for the admin-wall copy | yes |
| G11 | Microsoft admin-consent path and in-app ask-your-admin fallback | M | G1, G12 (owner action) | yes |
| G13 | Plain-chat write proposals: propose_provider_write tool and proposal cards (calendar event, sheet range edit) | L | G4, G7, G8, G9, G10 | yes |
| G14 | Save to OneDrive / Google Drive from generated-file cards, export menus and the artifact panel; replace-original | M | G7, G8, G9, G10 | yes |
| G15 | Contacts: read-only lookup for Outlook and Google | M | G10, D2 sign-off | yes |
| H8 | Deck path v1: research report to a designed .pptx with notes and charts, on the Terra route | L | H4 | yes |
| I4 | Settle a stream orphaned by a crash in seconds, not 15 minutes (3 to 5 days) | M | I3 |  |
| I10 | Persist regenerate and edit versions on the server and survive reload (about 1 week) | M | I9 |  |
| I11 | Checkpoint and rewind of the conversation, with explicit non-rewindable marking (about 1 week) | M | I9, I10 | yes |
| I17 | Canvas phase 1: highlight-to-edit, shortcut presets and restore this version (about 1 week) | M |  |  |

**Exit criteria:** - Every write goes through human approval plus a signed attestation. Always-ask is proven by unit tests, and the click path is proven live on Terra. - Terra generates an xlsx and one click saves it to OneDrive and Drive, with a receipt and verified read-back. A second click is idempotent. A stale-eTag replace returns 409. - A calendar event and a sheet-range proposal are applied by click on Terra. The card shows conflicts or before/after. Attendees are refused in v1. A =WEBSERVICE formula is refused. - The admin-wall card and consent sheet are proven on the business test tenant. - Do-mode agent writes are proven by mocks only. - G15 contacts stay off until counsel signs. - G6 edit-in-place passes the real-Excel check. - H8 passes the COM open check with no repair prompt. - I4: a hard kill settles in about 20 s with partial text kept. - I10 and I11: versions survive reload and rewind works.

**Needs from the owner:** OD14 (write posture), OD15 (integration:write grant), OD16 (publisher verification and test tenant), OD12 (business tenant), OD18 (binary staging, G6), OD19.

### Wave 6 - Deferred, owner-gated or low value (do not start)

**Goal:** Parked, each with a revisit trigger.

**Build order (tasks in the same group can be built at the same time):** E12, F12, H7, H11, I19, I20, I21, I23

| Task | What | Size | Depends on | Owner decision |
|---|---|---|---|---|
| E12 | Model-initiated browser_agent tool in Do runs, with a run-bound grant (parity item; cannot be proven live yet) | L | E2, E4, Do-mode test-route ruling (owned by the workstream that covers plan, wide research and agent tools; letter not given to me) | yes |
| F12 | Optional: the same reads in plain Ask mode through model-gateway tools (the only agent-style path that runs on Terra to… | L | F2, F3, F4, F6, F8, A (decides whether this is needed) | yes |
| H7 | Editable, re-runnable code cell (direct run, no model round trip) | L | H5 | yes |
| H11 | Post-write diagnostics for code artifacts instead of deploying an LSP bridge | S |  | yes |
| I19 | Read-only share link for a chat (real project, after the model decision) | L |  | yes |
| I20 | Copy a chat into a Space as a new Space thread (real project, after the decision) | L | I8 | yes |
| I21 | Server-synced project folders with folder instructions (about 1 week, after the precedence decision) | M |  | yes |
| I23 | Skills and learning loop: prove one skill authored from a real run and used in a later turn | M | delegated-identity workstream (letter not given) | yes |

**Exit criteria:** No work in this wave. Revisit triggers: - E12: after OD2 flips the Do default and the Do-on-Terra proof is accepted. - F12: only if A reports Do cannot run on Terra. - H7: after the H5 proof and verified sandbox CPU and memory limits. - H11: only on a concrete user need. - I19: after the share-link ruling and a security review. - I20: after I8 and a security review. - I21: after ADR-0003 precedence sign-off. - I23: step 1 (harness proof) can run any time after A0; step 2 waits for a user-bound reviewer route from D10.

**Needs from the owner:** OD19

### Workstream J (trust, data protection, owner-only items): sequenced separately

The first design for J came back empty, so the sequencer did not place it. It was re-run on 2026-10-01 and returned 16 tasks. Suggested placement: **J1, J2, J3, J9, J10** in Wave 0 or 1 (J1 and J2 are real defects in the mail-to-task path); **J11, J12, J13, J14** in Waves 1 to 3 (evidence packs and the residency line); **J4, J5** in Wave 3 once the owner has set up Postmark; **J7** with the owner's approval of a quiet run window; **J15** after J10 to J13; **J16** only if J12 confirms the erasure gap; **J6** parked until the unattended draft run (D10) exists.

| Task | What | Size | Depends on | Owner decision |
|---|---|---|---|---|
| J1 | Fix the mail-task replay guard so a failed delivery is never recorded as a duplicate | S |  |  |
| J2 | Calibrate the Postmark sender-verification against what Postmark really sends, and make a silent mass-drop visible | S | J1 |  |
| J3 | Prepare, dark, the policy principals, credentials and compose passthroughs for mail-to-task, plus a values-free readine… | M |  |  |
| J4 | MailTaskPipeline v1: post the synthesized first turn into the prepared mail_task thread (idempotent, framed as untruste… | L | J1, J3 | yes |
| J5 | Make mail_task threads visible and answerable in /chat (history listing, transcript, 'Opprettet fra e-post' banner, 'La… | M | J4 |  |
| J6 | Parked: unattended draft run for mail-task plus the 'drafted' notification (only after D10) | L | J4, J5, D10 | yes |
| J7 | Re-run the cross-tenant authority spec on the current build using the disposable harness (no owner passwords), and reco… | M |  | yes |
| J8 | Caption and clip-plan drafts, preview exports and a review sheet for the four product films | M |  | yes |
| J9 | Add a text alternative to the public recording player (silent video with captions only) | S |  |  |
| J10 | Data-flow map from code (by data class and route) plus a citation checker for all evidence packs | M |  |  |
| J11 | Sub-processor and recipient list derived from code, with a diff against the compliance pack and the /trust page | S | J10 |  |
| J12 | Retention and erasure behaviour with file:line evidence, including the per-user erasure question | M | J10 |  |
| J13 | Honest limits page and claims-versus-code reconciliation, with narrowing edits proposed for /trust | M | J10, J11, J12 | yes |
| J14 | Show residency, provider and tier per answer in chat, from server-stamped values only | M |  | yes |
| J15 | Assurance packet: counsel packet, audit scope options, pentest scope and rules of engagement, customer-managed-keys memo | M | J10, J11, J12, J13 | yes |
| J16 | Conditional: per-user erasure for Model Plane conversation data (only if J12 confirms the gap) | M | J12 |  |

**What the J design corrected in earlier documents:**
- The cross-tenant spec (`real-authority-knowledge.spec.ts`) does not need owner-created passwords: its harness (apps/Data Plane v2/tests/scripts/run-isolated-real-authority-browser.sh) builds a disposable stack with random per-run credentials. Its blockers are a macOS-only `stat -f %Lp` call, about nine image builds, and a last recorded pass on 2026-07-16. It stays an owner-approved run (J7).
- A public Trust Center already exists (apps/verevon-web/src/app/trust). It lags the code: no row for the ChatGPT-subscription route (global residency, no ZDR), nothing for several processors, and a text-to-speech residency line that depends on live configuration.
- Mail-to-task is built and off, but a failed delivery is recorded as a duplicate so Postmark's retries are dropped (J1), the sender check may drop every real email (J2), `MailTaskPipeline` is not wired, and a mail thread would not appear in /chat (J5).

**Owner checklist for J, in recommended order:**
1. Decide the mail-to-task questions: run-on-open versus unattended (recommended: run-on-open for v1), attachments (recommended: filenames only), notification (recommended: skip in v1), and who may use it (recommended: owner org only until Postmark is on the sub-processor list).
2. State, in one line, where customer data is hosted in production today and in which region. The compliance pack and /trust claim EU/EEA Azure hosting, and the repo has no deployment record.
3. Confirm whether the legacy Codex prompt logs (logs_2.sqlite and the legacy session transcripts on the integration-codex-subscription-home volume) have been deleted. If not, delete them yourself once the smoke turn shows no new logs file. Permanent deletion is owner-only; agents never do it.
4. Approve a quiet run window for the disposable cross-tenant harness (J7). No passwords are needed. Install any missing tool it reports (docker, pnpm, openssl, grpcurl) yourself.
5. Commission Norwegian privacy-counsel review of docs/compliance using the J15 packet. Nothing customer-facing is counsel-reviewed today; the status table stays unchanged until counsel records a review.
6. Choose and engage the external pentest vendor and set its scope from the J15 draft. Decide the certification path, body and compliance platform (recommended order: counsel, pentest, ISO 27001 plus ISO 42001, SOC 2 only on US demand).
7. Approve or edit the J13 /trust page diff (public content). Narrowing edits first; hold any broadening until counsel and evidence exist.
8. Create the Postmark account and a dedicated Postmark Server for inbound. Choose the receiving subdomain, add the MX record to Postmark's inbound host, generate the webhook secret (at least 32 random characters) into the launcher secret store, and set the inbound webhook URL with Basic credentials pointing at a path-restricted public route to integration-api (/api/v1/webhooks/mail-task/postmark only). Confirm Postmark's region, DPA and SCC terms and give them to engineering for the sub-processor list. Never paste secrets into chat.
9. After J1, J2 and J3 are merged and tested, set MAIL_TASK_PROVIDER=postmark, MAIL_TASK_WEBHOOK_SECRET and MAIL_TASK_RECEIVING_DOMAIN in the stack's local secret store (names only; values never in chat).
10. Apply the policy batch through the Control launcher: it adds model-gateway-mail-task-bridge and verevon-gateway and the integration-mail-task-worker credential, and restarts auth-core. Run check-mail-task-bindings.sh to confirm every binding is present. Agents never apply security settings to the running stack.
11. Approve the image rebuild window (launcher only; model-plane-only for Model Plane images, and the BFF rebuild batched at a wave end after a git status check), because J4 and J5 change session-core, model-gateway, the BFF and the frontend.
12. Send one real test email from your own verified address to your alias, then one from a different address to prove it is dropped. Share only the redacted header block (header names) so J2 can pin the sender-verification rule. Press 'Lag utkast' on the created thread (the Terra proof).
13. Confirm the Codex usage window has room for the roughly two proof turns, and refresh the local test sign-in if the machine rebooted. If capped, record it and stop; never switch model.
14. Do the editorial and media review of the four films: watch each short film and raw file with the draft captions, edit the captions, and sign each film in the review sheet. Then set the manifest approvals yourself. The manifest status stays blocked until every gate and approval is done.
15. Decide the open product questions that follow from J12 and J14: the residency-line default (recommended: always shown, from server values only), and treating a per-user erasure gap as a defect if J12 confirms it.

## 4. Critical path

Build chain:

1. A1 (RunAgent route, kill switch, proto number ledger) to A2 and A4 to A3 to the A6 free probes.

2. The Terra reset gate at 2026-10-03 17:45 UTC.

3. A6 live P3 to P5.

4. The B11, C10 and F13 proofs.

Longest serial lane, the execution-core agent.rs and mod.rs seams: A1, A2, A4, C1, C8, B4, B3, C3, C4, B5, F6, F8, G8. Put new logic in new modules to shorten it.

Other long poles:

- D3 to D4 (L) to D10 (L, adversarial security-review gate) to D13.

- E2 and E1 to E4 to E8 to E9 to E10 (L).

- B1 to B2 and B5 (L) to B7 (L).

- Gate for G writes: F6, F8 and F13 proven, then G1, G7, G8, G9, G13 and G14.

Quota gate: every live proof shares one Codex account window, which was exhausted on 2026-09-29. Proof ladder after a one-turn canary:

1. A6 P3 to P5.

2. H5 (five turns).

3. F10 (about 10 Ask turns).

4. E3, then E5 (about 25 to 30 calls).

5. H1 and H2 turns.

6. B11, C10, F13 and D13.

This totals roughly 150 Terra calls, so spread it across windows by priority. If a cap is hit, record it and stop; never switch model.

Hard ceiling: the 300 s delegated bearer. A runs about 270 s. B plan runs need 4+3n rounds at 10 to 40 s each, so even a 3-step plan overruns the 240 s clamp on Terra. B7's manual continuation or OD8's longer bearer decides whether B is useful on Terra. Wide research is capped at 6 to 8 items on Terra without OD8.

## 5. Files that several tasks edit

- execution.proto (RunAgentRequest and RunAgentResponse) and execution.pb.go. Tasks: A1, A2, A4, B3, B4, B5, C3, E12 (A7 merged). One proto steward owns the file and regenerates with buf. Never hand-merge: another session's uncommitted max_subagents=15 is in it. Number ledger: - Request: 15 max_subagents (taken), 16 provider_hint and 17 subscription_connection_id (A1), 18 max_rounds_per_child and 19 child_round_budget (C3), 20 authored_instructions (B3), 21 hook_context (B4), 22 approved_plan (B5), 23 wall_clock_secs (A4, consumed by C3 and C4), 24 allow_browser (E12; the design said 16, which collides with A1 and C3). - Response: 6 failure_code, 7 retry_after_ms, 8 resets_at (A2), 9 child_rounds_used, 10 children_started (C3). Batch additive scalars into one regenerate per wave. Each batch needs the Go wire-parity tests.
- sessions.proto and runs.proto. Tasks: B1 (SetRunModeRequest.plan_id=6), B3 (StartManagedRunResponse.persona_enabled), D3 (StartScheduledRunRequest.run_profile_json=10 and ScheduledStepContext.run_profile_json=6), D5 (RecordScheduledStepReceiptRequest 10 and 11), I8, I9, I10, I12 (BranchThread, SupersedeFromMessage, ActivateSupersedeBatch, SearchThreads). Same steward. Order: B1, D5, D3, B3 (Wave 1); I9, I8, I12 (Wave 4); I10, I11 (Wave 5). Session-core migration numbers: D6=0043, I9=0044, I12=0045, I19=0046. Re-check the directory when implementing.
- orchestration.proto, mp-orchestration proto_shim.rs, proto_wire_parity (Rust and Go) and proof_bundle.proto. Tasks: C7 (LineageEdge 5 to 7, batch_total), B5 (TransitionPlanStep), B9 (ApprovalProof.plan_id, RunProvenance), E10 (quarry_run_id). Order: C7 (Wave 1), B5 and B9 (Wave 2), E10 (Wave 3). One owner for the shim and parity tests.
- Every Model Plane proto change also needs buf generate, the mp-orchestration shim and parity tests where touched, and a cargo check of Data Plane v2. Tasks with proto changes: A1, A2, B1, B3, B5, B9, C3, C7, D3, D5, D10, E10, E12, I8, I9, I10, I12, I19. Rust here builds reliably only in Docker (one execution-core test hangs on Windows).
- model-gateway sse.rs. Tasks: A1, A2, A3, A4, B1, B3, B4, B5, B6, B9, C5, E12, H2 (hook), H6, I4, I6, I9, I16. Function-level ownership: - Normalize call, agentic branch and a pure build_run_agent_request: A1 first. Use a RunDispatchParams struct with defaults so later tasks add a field, not another positional argument. - AgenticRunOutcome::Failed arm: A2. - agentic_run_stream tail and deadline: A3. - spawn_run_dispatch budgets: A4 via a pure resolve_run_budget fn in run_budget.rs. B6 and C5 then add inputs. - Intake and PlanExecution: B1. - authored_instructions: B3. hook_context: B4. Order: A1, A2, A3 and A4, B1, B4, B3, C5, B6, B9. H2's sse.rs hook is two lines applied by the B-lane owner. Later waves go one at a time.
- execution-core runtime_loop/agent.rs. Tasks: A1, A2, A4, C1, C8, B4, B3, C3, C4, B5, F6, F8, G8. Order: A1 (InferRequest route, resolve_agent_model, provider guard), A2 (InferFailed, LoopContext slot, finalize), A4 (loop wall clock), C1 (child registration), C8 (child tools), B4 (hook_context literal), B3 (system insert), C3 (child budget, run_agent_with_tools), C4 (fan-out guards), B5 (run_agent, Phase 1 and 3, recitation), F6 and F8 seams (offered_tool_defs, record_tool_step, data_category), G8 (approval_reason_for). Put new logic in new modules (run_budget.rs, credential_deadline.rs, subscription_refusal.rs, connector_reads.rs, a plan checklist module) and touch agent.rs only at seams. Re-read before every edit. Never revert others' hunks.
- execution-core runtime_loop/mod.rs, integration_tools.rs, permission/mod.rs and capability_policy.rs. Tasks: B5 (plan.step_status dispatch arm), B8 (execute_provider_action verification), F6 (read_provider_action arm, catalog), F8 (render seam), G8 (approved mode, always-ask), G4 and G15 (catalog entries). Order: B8, F6, F8, B5, G4, G8. F6 adds its dispatch arm first.
- execution-core subagent/research_fan_out.rs (untracked WIP of another session) and subagent/mod.rs. Tasks: C8, C3, C4. Needs the OD1 snapshot first. Order: C8, C3, C4.
- model-gateway normalize.rs. Tasks: A1, B1, C5 (and the G13 feature flag). Treat it as one subscription-route policy table owned by A1. Still rejected on the subscription route: zdr, attachments, browse_web, deep_research, generate_image, client tools, structured schema. Agentic is allowed only with the switch on and plan_mode true. B1 adds execute_plan_id allowed only when AGENTIC_SUBSCRIPTION_ALLOW_EFFECTS is on (needed for B11; B1's design says refuse). C5 adds wide_research, refused until C3 and C5 land and OD7 is answered.
- model-gateway tool_loop.rs. Tasks: F8, F11a, G6, G13, H5, H6, H11, I6, I12, I13, I16. Do not merge the inline loop with the agentic loop (apps/Model Plane/CLAUDE.md). Order: H5 (one-line tool description), F8 and F11a, H6 (code_interpreter arm with a shared merge function), I6, I13, I16, I12, G6 (staged binary reuses H6's merge; binary wins on a name clash), G13. New tools go in new modules with one-line seams.
- model-gateway http_routes.rs (additive-seam file). Tasks: B1 (InvokeRequest.execute_plan_id, plan_approval reorder), C5 (InvokeRequest.wide_research), C7 (lineage_value), B9 (proof_approval_value), D6 (cron routes), I3 (serve loop), I8 to I12 (routes). Small Edit calls on distinct functions. Add the InvokeRequest fields of B1 and C5 in one edit each, re-reading between.
- model-gateway prompt_context.rs and format_guidance.rs. H2 creates the format_guidance slot (chart notice); H6 and H8 reuse it. The byte-identical-prompt tests on the saved journey prompts guard every notice.
- FE chat-client.ts buildChatWireBody. Tasks: A5, B2, C6, G6, G13, I2, I7, I13, I16. A5 first splits it into small per-concern helpers (a subscription policy table of what is dropped, feature list, extras) so later tasks add rows. Order: A5, B2, C6, I2, I7, I13, I16, G6, G13.
- FE DashboardComposer.tsx, by region. Do control: A5, C6. Commands and commandNeedsFullRoute: I7 owns and makes it table-driven; I13, D11 and E9 add flags. Connector picker: F5, F9, G5, G15. Screenshot: E13. One owner per region per wave. Another session holds 14 uncommitted lines. Never rewrite the file.
- FE use-chat-controller.ts, ChatMessages.tsx, ChatPage.tsx, chat-types.ts, chat-normalizers.ts. Tasks: B2, B7, C6, C7, D8, D9, E9, E10, G13, G14, H2, H3, I2, I4, I5, I8 to I11, I13, I17. New components go in new files with one-line mounts. Additive state and functions only. Owners by wave: W1 D9, H2, C7; W2 B2, D8; W3 B7, C6, E9, E10; W4 and later the I-items. Re-read before each edit. The frontend container serves live source with no hot reload: git status first, verify on :5199.
- BFF apps/gateway. agent_actions.rs: D3, then D6, then D11. spaces.rs: D2, then B1 strip, then I21. browser.rs: E1, then E4, then E9 and E11. chat/shared.rs: E1, E10, C9. history.rs: B1, I8 to I11. A BFF rebuild restarts the frontend container, so batch BFF changes at wave ends (OD20).
- session-core grpc.rs, orchestration_grpc.rs, orchestration_store.rs, auth.rs. Tasks: B1, D3, D5, D12, B5, B9, C7, I8 to I12. Order: B1, D5, D3, D12 (Wave 1). B10 tests go first on orchestration; then C7, B5, B9. One session-core owner per wave.
- plane-service-principals.json, run-control-plane.sh, auth-core plane-token.controller.ts and convex-token.service.ts. Tasks: D1, D10, E11, F1 (probe only), F7, F12, G8, C9 (A8 merged). One policy steward. The OD1 snapshot of the other session's delegated-identity edits comes first. Batch grants per wave. Each grant gets a security review and an auth-core contract spec update. Applying to the running stack is an owner action through the Control-plane launcher. Policy invariant test: the schedule-step principal can never mint integration:write, browser grants or the approved permission mode (draft-only rule). Order: D10 then C9 (same controller file).
- Model Plane docker-compose.yml (additive-seam file). Tasks: A0, A1, A4, C3, D1, E2, E4, F6, F7, I3. Append-only env blocks, small Edits, one editor at a time. Env ledger: AGENTIC_SUBSCRIPTION_ENABLED and AGENTIC_SUBSCRIPTION_ALLOW_EFFECTS (same expression in model-gateway, execution-core and inference-core), AGENTIC_MAX_ROUNDS, AGENTIC_MAX_SUBAGENTS and the AGENTIC_SUBSCRIPTION_* variants, EXECUTION_RUN_WALL_CLOCK_SECS, LEARNING_REVIEW_ENABLED, QUARRY_BROWSER_AGENT_LLM (default off until E5), PROVIDER_READ_TOOL_ENABLED (default off), WIDE_RESEARCH_*.
- integration-corev2 actions/service.go and service_test.go, catalog.go, oauth/service.go. Tasks: F2, F3, F4, F5, G1, G4, G7, G15. One new file per op family (mail_ops.go, calendar_ops.go, files_ops.go, workbook_ops.go, write_ops.go, contacts_ops.go); service.go gets dispatch lines only. F2 lands pagetoken.go first. Calendar read is F3 only (G4's calendar.view and Microsoft calendar bundle are cut: F3 and F5 own them). G1 owns the reconnect union that F5 relies on.
- imports-core (parsers.py, chat_extract.py, auth_middleware.py, Dockerfile, config.py, main.py). Tasks: H1, F7, H9, H10 (G2 and I14 merged). Order: H1 (zip guard, truncated flag), H10, F7 (max_chars, imports:extract), H9. H9 uses an 18 s OCR deadline; I14's 45 s budget exceeds the 25 s BFF extract timeout. Agree one limit set (caps and truncation marker) once.
- model-gateway browser_run.rs, execution-core llm_planner.rs, browser_agent.rs, tool_bridge, and BFF browser.rs. Tasks: E2, E1, E4, E8, E10 (A7 merged into E4). Order: E2 and E1 deploy together, then E4, E8, E10. E3's schema result feeds E4. Update the five PlanConfig literals together.
- chat-export (pptx.ts, xlsx.ts, docx.ts, index.ts) and chat-chart/geometry.ts. Tasks: H2 (spec, geometry), H3, H4, H8. H2 first. H4 then H8 share the chart spec and pptx.ts.
- capability-core Go (workflow_dispatcher.go, completion_consumer.go, executor.go, workplane_apis.go, runwatch). Tasks: D6, then D7, then D11. A0 touches only cmd/main.go and sessionreview.
- Dated evidence docs (CHAT_PAGE_GAP_PLAN, CHAT_PARITY_AUDIT, PRODUCT_RECORDING_READINESS, MODEL_PLANE_RELEASE_EVIDENCE, ledger.md). Tasks: A6, B11, C10, D13, E5, E6, F10, F13, H5 and others. One scribe appends dated rows. No concurrent edits. Record counts and ids only, never tokens or message content.

## 6. Start now (no owner decision needed)

1. **A0**: Stop paid-provider leakage before any Terra live run (background learning review) (S)
2. **A1**: Carry the subscription route through RunAgent: proto, gateway, execution-core, kill switch (M)
3. **F1**: Make delegated identity verifiable and the owner-only tests runnable (S)
4. **B10**: Run the plan-gate SQL against real Postgres (baseline and regression net) (S)
5. **D2**: Fix template digest parity between the BFF and Go before adding fields (S)
6. **F2**: integration-corev2 mail: real search, metadata, folders, paging and attachment metadata (Outlook and Gmail) (M)
7. **G1**: Make scope upgrades additive and truthful (precondition for every new bundle) (S)
8. **H1**: Extractors that read what Excel and PowerPoint actually store (formulas, dates, notes, tables, charts, zip guard) (M)
9. **H5**: code_interpreter on Terra: advertise the whole library set, probe imports, first live proof of docx, pdf, pptx, xlsx and a chart (S)
10. **D9**: 'Varsle meg' control in the waiting state and the live run panel (S)
11. **E7**: Risk classifier hardening: Norwegian markers, password-field detection, regression table (S)
12. **B8**: Post-mutation verification inside the loop (provider writes and shipment booking) (M)
13. **F3**: integration-corev2 calendar read: windowed, searchable, trimmed, in local time (Outlook and Google) (M)
14. **F4**: integration-corev2 files and SharePoint: libraries, folders, paging, Drive shared drives, and the missing Microsoft drive tests (M)
15. **H2**: Native chart renderer in chat: chart fence, table-to-chart, copy and CSV, steering notice (M)
16. **I3**: Redeploy-safe turns: graceful drain, stop grace period and REDIS_URL on model-gateway (1 to 2 days) (S)
17. **D12**: Close the org-wide read of system-owned scheduled threads and verify the personal-Space read live (S)

## 7. Owner decisions (33), each with a recommended default

**OD1. Snapshot the uncommitted shared work before Wave 1: untracked research_fan_out.rs and ChatWideResearchPanel.tsx, the delegated-identity files (auth-core plane-token.controller.ts, plane-service-principals.json, run-control-plane.sh, integration-corev2 auth files) and regenerated pb.go files? Agents never commit, stash or revert.**
- Options: Owner commits or makes a WIP snapshot first / continue in the working tree
- Recommended: Snapshot now. First confirm the real checkout is C:/dev/CoresSystem (the session cwd is not a git repo, and OneDrive near-duplicates exist) and scan for NUL-filled files. One more abrupt shutdown would lose the untracked files.
- Blocks: C3, C4, C7, C8, D1, D10, F1 step 5, F7, E11, every proto batch

**OD2. Enable Do mode on the ChatGPT subscription route (AGENTIC_SUBSCRIPTION_ENABLED), may it run effects, and should a test-only stub route exist? (Merges A's two decisions, the Do-mode test-route questions from E, F, G and C, and B11's need for an executing run.)**
- Options: Switch off everywhere / on only in the proof stack / on by default. Read-only (plan_mode forced) / effects with ask posture. Stub route or none.
- Recommended: Code and compose default OFF; ON only in the proof stack for A6. Read-only for the first proof, enforced server-side. Allow effects on the proof stack only when B's executing-run proof needs it: AGENTIC_SUBSCRIPTION_ALLOW_EFFECTS, ask posture, per-call approvals, and B1 accepting execute_plan_id on subscription only then (B1's design refuses it, which would make B11 impossible). Flip the compose default after the dated live proof and your acceptance. No stub route: the real subscription path is exercised and the free negative probes cover the rest.
- Blocks: A1 enablement, A5 UI, A6 live, B11, C10, F13, E12, G Do-mode paths

**OD3. Quota guard and run budgets for Do runs and wide research on the subscription route (the plan hit its usage cap on 2026-09-29).**
- Options: Rounds 4 / 8-10 / more; fan-out concurrency 2 / 3 / 4; one Do run in flight per user; wide ceiling 24 or 100
- Recommended: First proof: rounds 10 for the whole run (parent plus children), fan-out concurrency 2, wall clock 240 s, one Do run in flight per user. Then rounds 8 and concurrency 3. Wide-research defaults, all env-tunable and replaced after C10 measures: ceiling 24 items (hard cap 100), 4 rounds per item (cap 6), at most 400 child rounds, parent max_rounds 6, breaker after 3 provider failures, 3-wide on subscription and 4 on metered. Raise the subscription round cap on the proof stack for B11 only, because plan runs need 12 or more rounds.
- Blocks: A4, B6, C3, C4, C5

**OD4. May the background skill-learning review send a subscription-route run's transcript to a paid model (it calls claude-sonnet-4-5 with a service credential today)?**
- Options: Never / only on the user's own route / yes
- Recommended: Never. Disable it on the proof stack now (A0). Permanent rule: skip any thread that ran on a user-owned subscription route until the review can run on that user's own route with a delegated credential. It is both a standing-rule leak and a data-protection issue.
- Blocks: A0 permanent rule, I23

**OD5. Terra proof window and quota spend: confirm the Codex reset, approve the proof ladder, and allow a refresh of the local test sign-in (needed after a reboot).**
- Options: One window / split across windows by priority / wait
- Recommended: Canary turn first to confirm the reset (owner memory says 2026-10-03 17:45 UTC). Then run the ladder in the critical_path order and spread about 150 calls across windows by priority. If capped mid-proof, record the cap and stop; never switch model or use Verevon Balance.
- Blocks: Every live proof (A6 P3-P5, E3, E5, F10, H1, H2, H5, B11, C10, D13, F13)

**OD6. Plan-gate defaults (B): does approval pre-authorize effects, does Approve auto-start the run, how does the plan continue after an approval pause, what is the round budget, what is recited, and where do hooks apply?**
- Options: (1) Rung only caps, each effect still asks / (2) pre-authorize kept operations / (3) pre-authorize all within the rung. Auto-start or a 'Kjor planen' button. Manual or automatic continuation. Plan-derived rounds or a global raise. Approved-plan recitation only or a model todo. Hooks on the agentic loop only or the inline loop too. Inline plan gate or wait.
- Recommended: Option 1 (each effectful action still asks). Auto-start with a visible user message, and keep the button as the retry and reload state. Manual 'Fortsett planen' in v1. Plan-derived rounds for plan runs only. Recite the approved plan only. Hooks on the governed agentic loop only (record the ruling in docs/decisions/ledger.md). No inline plan gate; wait for Do on Terra.
- Blocks: B1 normalize behaviour, B2, B4, B6, B7

**OD7. Wide-research design (C): budget semantics, entry point, who may use it and the org limit.**
- Options: Pool from parent rounds with a raised ceiling / separate child budget with even division kept / leave as is. Composer toggle without a confirm / with a confirm step / model-chosen only.
- Recommended: Separate child budget, even division kept, parent charged 0 rounds. A composer toggle in Do mode with 12/24/48 presets and no confirm gate in v1. All non-ZDR members may use it. The org admin may set max_subagents_per_run; unset means 24 on wide turns and 8 otherwise. WIDE_RESEARCH_FAN_OUT_ENABLED stays the deployment kill switch. The honest Terra ceiling is 6 to 8 items without OD8.
- Blocks: C3, C5, C6

**OD8. Run-length credential: allow a longer-lived delegated bearer class (up to 1500 s, hard cap 1800 s) beyond the 300 s default? Generalised from C9; the same 300 s ceiling bounds A, B, C and E.**
- Options: Wide-research turns only / every agentic, plan and browser run / keep 300 s
- Recommended: Yes for wide-research turns only, after a short security review, and only if C10 or B11 show real runs crossing 240 s. Do not generalise yet. Otherwise accept 6 to 8 items on Terra and use B7's manual continuation for plans.
- Blocks: C9, C4 window checks, B7 and B11 viability, E run length

**OD9. Scheduled runs (merges A's and D's decisions): when to apply the scheduled-lane policy (D1), may a scheduled step act as the schedule owner on the subscription route (D10), what may it read, where does the owner see the result, what are the cadence and failure rules, and what is the notification surface? A8 is folded into D10.**
- Options: Apply now / after D4 merges / not at all. Token exchange bound to the Control decision (a) / inference-core verifies the decision itself (b) / no owner identity (c). Web only / plus knowledge / plus personal connectors. Read-only transcript plus 'Fortsett i chat' / user-owned delivery thread. Bell with link only / preview / push or email. Fail visibly / fall back to a managed model.
- Recommended: Prepare D1 now; the owner applies it after D4 merges, in the same window as the D4 build. D10 via option (a): limited to inference-core, 120 s, subscription route only, with an adversarial security review before activation. v1 reads web only and stays draft-only: a read-only tool allowlist, and a NeedsApproval step fails the run. The owner reads the result through the personal-Space transcript plus 'Fortsett i chat'. Floor of 1 hour for non-admins, 10 active schedules per creator, auto-pause after 3 consecutive failures, 'Kjor na' once per 10 minutes. The bell shows title and link only. Fail visibly with no fallback to Balance. No scheduled run spends a personal plan until D10 ships.
- Blocks: D1 apply, D10, D11, D13

**OD10. Agent reads of personal data (F): may the agent read the run owner's own mail, files and calendar without a per-call approval? Also ZDR, egress after flagged content, Ask-mode tools, the imports:extract scope, the calendar scope path and a SharePoint picker tab.**
- Options: Autonomous own-data reads / one consent per run / per-call approval. Refuse ZDR / allow. Ask-mode read tools (F12) now or not. Approval for egress tools after a Flagged read, yes or no. imports:extract narrow scope / text-only / extraction in integration-corev2. Calendars.Read via the connect path only / also at sign-in. SharePoint picker tab now or not.
- Recommended: Autonomous own-data reads through read_provider_action (a LOW capability) behind PROVIDER_READ_TOOL_ENABLED, default off, with untrusted-content framing and durable redaction. Refuse ZDR. Do not build F12. Before any customer org uses it, require approval for egress tools after a Flagged read (F8b). imports:extract only on the extract path, never imports:write. Calendar through the connect path only, not sign-in. No SharePoint picker tab now.
- Blocks: F6, F7, F8, F13, F12

**OD11. Shared inbox vs owner-only personal connections. Today emailsync polls personal Microsoft and Google connections into an org-wide inbox that inbox_search and the Inbox UI show to every member. This contradicts the owner-only ruling.**
- Options: (a) keep org-wide / (b) connection visibility 'personal' or 'org' with an explicit share choice at connect time / (c) tag conversations with their owner and filter
- Recommended: (b), with existing inbox connections switched to org only on your say-so. Interim: stop syncing personal-provider connections into the shared inbox unless they are explicitly shared. Until then, state that inbox_* is org-wide and do not point F6 at it.
- Blocks: F11b, F11a copy, F13 negative test

**OD12. Accounts and tenants for live proof of connectors, writes and consent. OAuth consent is an owner action.**
- Options: Dedicated test accounts / production mailboxes
- Recommended: A dedicated Microsoft 365 developer tenant with a SharePoint site and Sites.Read.All consented, plus a dedicated Gmail account, seeded with fixed fixtures. For G11, a business test tenant with the default consent policy, one non-admin member and one admin. Never production mailboxes.
- Blocks: F10, F13, G4 and G5 live, G11, G13, G14

**OD13. Hosted-browser run rules (E): domain scope and who owns the policy, whether the answer is written into the thread, whether the planner may run on the user's subscription, how the user owns the agent's Quarry run, and the planner default.**
- Options: Fence of start host plus listed domains / plus an org allow-list / open web with a deny-list. Write the answer to the thread / keep it in the panel. Planner on the subscription when selected / Balance only. User's delegated quarry bearer / service principal with a broker proof. QUARRY_BROWSER_AGENT_LLM default on or off.
- Recommended: A hard fence of the start host plus domains the user lists, with the broker as policy authority (deny-list, at most 8 domains). Write the planner's result and source URLs into the thread, non-ZDR only, never raw page text. The planner may run on the user's subscription only when that route is selected; the default stays verevon-balance for everyone else. Run the agent with the user's delegated aud=quarry bearer, which is bounded by the 300 s lifetime and the 240 s clamp. Keep QUARRY_BROWSER_AGENT_LLM default OFF in compose until E5 passes, so no manual AI-loop run silently moves to the paid planner. The user's own Chrome is never used.
- Blocks: E2, E4, E8, E10, E12

**OD14. Write posture for calendar, Excel and Sheets, and file saves (G): always ask, calendar attendees, where files go, Google scopes and contacts.**
- Options: Always ask every write / ask once per chat for creates / trusted after N clean runs. No attendees in v1 / with a second confirm / freely. New file in OneDrive > Verevon / always pick a folder / overwrite by name. Google spreadsheets plus drive.file / drive.file with a picker / full drive. Contacts read-only lookup behind a flag off until counsel signs / sync / skip.
- Recommended: Always ask for every write, including overwrite, delete and the existing mail.send and gmail.send, which self-approve today under the auto posture in chat Do runs. No attendees in v1. New file in OneDrive > Verevon with rename on conflict. Replace-original only for a picked file, with an eTag guard and a second confirm. Google spreadsheets plus drive.file. Contacts lookup built behind a flag and kept off until counsel signs. G7 capabilities stay unrequested until G8 is deployed.
- Blocks: G7 to G15

**OD15. Widen the model-execution service principal with integration:write for the ingestion audience (G8), or use a dedicated presenter principal? Also the policy-batch process: grants are prepared in source and the owner applies them through the Control-plane launcher.**
- Options: Grant model-execution integration:write for ingestion only / dedicated presenter principal
- Recommended: Grant model-execution integration:write for the ingestion audience only, with a security review. The signed attestation bound to the exact payload, the owner-only check on the attestation actor, the idempotent receipt and always-ask are the real gates. The exchanged delegated token must still never mint write. Agents prepare the policy change in source; applying it to the running stack is an owner action.
- Blocks: G8, G9

**OD16. Business-tenant consent and publisher verification (G10, G11, G12).**
- Options: (a) publisher verification only / (b) admin-consent link plus an in-app ask-your-admin card / (c) split personal-use scopes from org-sync scopes in the default connect / (d) lighten the Microsoft login scopes
- Recommended: Do (a), (b) and (c). Decide (d) before onboarding the first non-admin business tenant. Start now: MPN id, verified publisher domain on both Entra app registrations, redirect URIs, and Google OAuth verification status. These have external lead time (G12).
- Blocks: G10, G11, SharePoint live proof

**OD17. Should synced SharePoint and OneDrive Office files (docx, xlsx, pptx) enter the org knowledge base by default (G3, with F14 folded in)?**
- Options: Default off, flag per environment / default on
- Recommended: Default off (FINSPO_EXTRACT_OFFICE=false) until you approve. FINSPO_CAPTURE_CONTENT is already default-on, so enabling it would index most library content. The user-initiated xlsx/pptx knowledge import (H10) needs no decision.
- Blocks: G3

**OD18. Office and data output choices (H): PDF fonts, OCR engine, attachment caps, truncation, deck theme, binary staging, the editable code cell, and H11 and H12.**
- Options: Standard 14 fonts / embedded Unicode subset. Local Tesseract / Azure Document Intelligence. Keep 1 MB / raise to 4 MB. Truncate with a marker / refuse. Neutral theme / brand tokens. Hold uploaded binaries in memory for the run or not. Editable cell with a flag / model round trip. Skip H11 and H12 or build.
- Recommended: Standard fonts with transliteration. Local Tesseract (nor+eng) with an 18 s deadline, 15 pages and no vendor. Raise the cap to 4 MB for xlsx, pptx and pdf after H1's zip guard lands; the 60k character limit stays. Truncate with a marker on the chat path and refuse on knowledge import. Neutral deck theme with a DeckTheme seam. Binary staging held in memory only after H5 and H6 prove the sandbox on Terra (G6). Editable cell behind a flag, default off, after sandbox limits are verified (H7). Skip H11 and H12.
- Blocks: H3, H9, H10, H1 limits, G6, H7, H11

**OD19. Chat-core rulings (I): share-link model, presence, duplex voice, copying a chat to a Space, projects, rewind scope, reasoning default, usage-meter ceiling, selection-edit scope, and who may author slash templates.**
- Options: Public / named recipients / org-internal snapshot. Presence in /chat or Spaces only. Governed duplex voice or park. Copy / move / nothing. Per-browser folders / server-synced folders with instructions / private-room projects. Conversation only / plus artifact versions / plus workspace state. Hidden / collapsed summary / expanded reasoning. Show only / warn / stop mid-run. Phase 1 only or a server range patch. Users only or users and org admins.
- Recommended: Org-internal sign-in snapshot link, 30-day expiry, revocable, redacted text view. Presence stays in Spaces. Park duplex voice, keep governed push-to-talk, and run one real-microphone check yourself on Chrome and Safari. Copy into a Space as a new Space thread, never move. Server-synced folders with a project-instruction layer under the persona (ADR-0003 sign-off needed). Rewind is conversation plus artifact versions, and effectful turns are non-rewindable. Reasoning shown as a collapsed summary only when text arrives, with effort unchanged. Usage meter shows and warns at 80% in v1. Selection edits stay at phase 1 (I18 is cut). Templates are user-authored first.
- Blocks: I22, I19, I20, I21, I11, I13 phase 2, I15, I16, I17

**OD20. Frontend and BFF rebuild coordination: a BFF rebuild restarts the frontend container, which serves live source with no hot reload, while another session holds uncommitted frontend edits.**
- Options: Batch rebuilds at wave ends after a git status check / rebuild per task
- Recommended: Batch BFF and frontend rebuilds at wave ends after a git status check, and verify frontend work on the :5199 host preview in between. Model Plane images through the launcher with LAST_STACK_INDEX=2 do not restart the frontend. Build only through the launcher; hand-built docker compose misses the Control Plane env chain.
- Blocks: A5, B2, C6, D8, D9, E9, F9, H-FE items, I-FE items

**OD-J1. Mail-to-task v1: when is the draft produced?**
- Options: (a) Run on open: the email becomes a thread with the framed first turn, and the user presses 'Lag utkast' (one draft-only Ask turn on their own Terra route). (b) Unattended: a draft exists before the user opens the thread, which needs D10's unattended owner identity and a security review.
- Recommended: (a) for v1. It needs no new identity, no paid spend and no unattended model call, and it can be proven on Terra. Build (b) as J6 only after D10 passes its security review. State plainly that v1 creates a thread and a prompt to draft, not an unattended draft.
- Blocks: workstream J

**OD-J2. Receiving domain, ingress and Postmark setup shape (provider is already ruled: Postmark, self-forward only).**
- Options: A dedicated subdomain (for example tasks.<your domain>) with MX to Postmark inbound on its own Postmark Server, versus reusing a main-domain address. Ingress: a path-restricted tunnel or reverse proxy to integration-api for /api/v1/webhooks/mail-task/postmark only, versus exposing the whole service. Webhook auth is Basic credentials embedded in the webhook URL (verify.go:11-24).
- Recommended: A dedicated subdomain, its own Postmark Server, a path-restricted public route for that one path only, and a webhook secret of at least 32 random characters held in the launcher secret store. Pilot on your own org and your own verified address first. The domain name is your choice.
- Blocks: workstream J

**OD-J3. Postmark is a new sub-processor of forwarded email content. Who may use mail-to-task, and when?**
- Options: Owner org only until the sub-processor list, transfer assessment and /trust page include Postmark and its region and DPA status are confirmed; versus enabling for all non-ZDR orgs now.
- Recommended: Owner org only. user-core already refuses ZDR orgs. Enable for customer orgs only after Postmark (region, DPA, SCCs) is in docs/compliance/subprocessor-list.md, transfer-assessment.md and /trust, and counsel has seen it.
- Blocks: workstream J

**OD-J4. Attachments in v1.**
- Options: (a) Filenames listed, text not read. (b) Extract text, which needs a service identity that imports-core accepts (an RS256 'ingestion' JWT; attachments.go:70-95) plus an auth-core principal.
- Recommended: (a) for v1; extraction is a separate small follow-up. xlsx and pptx stay unextracted until workstream H's H1 and H10 land.
- Blocks: workstream J

**OD-J5. Notification when an email thread is created.**
- Options: Skip in v1, or provision a NATS publisher identity and ACL for the gateway and publish content-free events.
- Recommended: Skip in v1. The thread appears in the /chat sidebar with a 'Fra e-post' badge (J5). Add the notification with J6, when a real draft exists, so it never says 'drafted' without a draft.
- Blocks: workstream J

**OD-J6. Order and scope of third-party assurance.**
- Options: Counsel review, external pentest, ISO/IEC 27001 (Norwegian market norm) with ISO/IEC 42001 in the same cycle, SOC 2 Type I then II, or any subset.
- Recommended: 1) Commission counsel review of docs/compliance now (it gates everything customer-facing). 2) Commission an external web-app and API pentest including multi-tenant tests, with a published summary. 3) Engage a compliance-automation platform and a certification body for ISO 27001 scoped to the platform, adding ISO 42001 once the management system exists (it matches the Trust Center's stated spearhead). 4) SOC 2 only when a US customer asks. Choose the vendors and budget yourself; engineering provides the J15 packet.
- Blocks: workstream J

**OD-J7. Public claims policy until assurance exists.**
- Options: Never claim parity with Microsoft Enterprise Data Protection or the EU Data Boundary; claim only the specific enforced controls, and publish narrowing corrections to /trust before counsel review while holding any broadening until counsel and evidence.
- Recommended: Adopt that rule. The defensible sentence today is 'residency and ZDR are enforced per request on the routes that support them; no third-party certification yet'. Approve the J13 diff before it goes public.
- Blocks: workstream J

**OD-J8. Residency line per answer (J14): who sees it, and how is the subscription route labelled?**
- Options: Always shown to every user, shown only to admins and ZDR orgs, or not shown. Subscription route labelled 'global, ingen EU- eller ZDR-garanti' versus left blank.
- Recommended: Always shown as one small line, from server-stamped values only, hidden when unknown. Label the ChatGPT-subscription route 'global' with the plain no-EU, no-ZDR wording, because that is what the code says (codex_subscription.rs:607-626).
- Blocks: workstream J

**OD-J9. Customer-managed keys, self-host and sovereign offer.**
- Options: Build nothing now and say so; per-org envelope encryption with a customer-held key; or the sovereign tier path.
- Recommended: Build nothing now. State it in the limits page. Revisit when a signed contract requires it. The J15 memo gives the options and a rough scope.
- Blocks: workstream J

**OD-J10. Where does production run, and is the EU/EEA hosting claim true today?**
- Options: State the facts in one line: hosting provider and region for customer data, versus keep the current wording.
- Recommended: State the facts. The repo evidences a local Docker stack and production compose overrides but no deployment record, while docs/compliance and /trust say EU/EEA Azure hosting. Until stated, the limits page describes the environment as it is and /trust makes no claim beyond the evidence.
- Blocks: workstream J

**OD-J11. Cross-tenant spec (J7): approve running the disposable harness on this PC?**
- Options: Yes in a quiet window (no Terra proofs running), versus use a separate Linux machine, versus skip.
- Recommended: Yes. No passwords are needed; the harness generates random per-run credentials on loopback and deletes everything. It builds about nine images and needs docker, pnpm, openssl and grpcurl. Installing any missing tool is yours. Engineering ports the macOS-only stat call first.
- Blocks: workstream J

**OD-J12. Product films: editorial rules and who signs.**
- Options: Owner watches each short film and raw file and edits the captions. Keep action-description captions on silent video plus a text alternative (J9), versus captions only.
- Recommended: You sign each film. Captions describe visible actions only, disclose every time cut, and make no speed or correctness claim beyond what the markers prove. Keep the 'fictive data' label and add the transcript. One film per scenario from the final pass build.
- Blocks: workstream J

**OD-J13. If J12 confirms that per-user erasure does not delete a user's Model Plane threads and memory, how urgent is it?**
- Options: Treat as a defect and fix before any DPA claims erasure completeness, or document it as a limitation.
- Recommended: Treat it as a defect (J16), and until fixed say in the limits page that per-user erasure of conversation content is not evidenced end to end.
- Blocks: workstream J

## 8. What was cut or deferred, and the risks

MERGED (one owner, not built twice):

- A3 and C2: A3 owns the agentic stream follow. On deadline it keeps the retryable agent_dispatch_unavailable and adds run_id. C2's non-error run_still_running frame and the SPA reattach are deferred until C10 shows runs over 270 s.

- A7 into E4: E4 threads the planner route; E12 later passes the run's route.

- A8 into D10: A8 was a design item and is not built separately.

- G2 into H1 and H10: same parsers.py and allow-list.

- F14 into G3: same extract.go, flag default off.

- I14 into H9: I14's 45 s OCR budget exceeds the 25 s BFF extract timeout. H9's 18 s deadline is correct.

- G4's calendar.view and Microsoft calendar bundle into F3 and F5.

- F5's reconnect union into G1.

- The A4, C3 and C4 wall clocks collapse into one run_budget module plus one expiry-aware guard.

- The A4, B6 and C5 round budgets collapse into one pure resolve_run_budget function.

- A2's typed refusal feeds C4's breaker.

CUT:

- E14: cannot be proven on Terra (supports_vision=false, attachments rejected there).

- I18: a review change that does not meet the critic's test plan.

- H12: needs a Linux spike and the effectful half is not planned.

DEFERRED to Wave 6, with triggers in that wave: E12, F12, H7, H11, I19, I20, I21, I23.

Flagged exceptions to the rule that G and I come after B to F: G1 (a consent-correctness fix F5 needs, not a new row) and I3 (protects scarce Terra proof turns from launcher redeploys). Everything else in G and I waits for Wave 2 exit.

SIZE FLAGS (understated):

- A1 M to L: proto, three services, regeneration of a dirty pb.go, the Data Plane v2 check, Docker-only Rust builds.

- B1 M to L: nine files across four services.

- B2 M to L.

- C3 M to L: rewrites pool semantics and about five tests in a file under concurrent edit.

- C7 M to L.

- D3 M to L: eleven files, five services, two protos.

- D6 M to L.

- E2 M to L.

- E4 M to L: eight files and five PlanConfig literals.

- E9 M to L.

- E10 L to XL.

- G7 L to XL.

- G13 L to XL.

- H3 L to XL: it is a PDF layout engine. Slice it: text, lists and tables first, charts after H2 and H4. H5's reportlab path already yields a model-driven PDF.

- H4 M to L: ship Excel first, then cached-values-only Word and PowerPoint.

- I9 M to L: every history read path must filter superseded rows. Write the failing tests first.

- F3 and F4 are M but rest on unverified Graph behaviour: $search with $filter, Prefer timezone names, calendarView. Capture real responses in F10 and turn them into fixtures.

STANDING-RULE FLAGS:

1. A0: capability-core's learning review sends non-ZDR run transcripts to claude-sonnet-4-5 (a paid provider) and may already be happening on Terra chats. Fix first.

2. B1 refuses execute_plan_id on subscription and A1 is read-only, so B11 (the executing run) cannot be proven on Terra unless effects are allowed on the proof stack with the ask posture (OD2). The normalize.rs policy table resolves it.

3. E4's compose default QUARRY_BROWSER_AGENT_LLM=1 would move every manual AI-loop run to the paid Balance planner. Keep it off until E5 passes. E5 must pin Terra with a route guard.

4. The scheduled lane runs on Verevon Balance until D10, so D13 cannot run live before D10.

   - Keep scheduled runs draft-only: add a policy test that the schedule-step principal never mints integration:write, browser grants or the approved permission mode.

   - G8's approved mode is accepted only on the ExecuteStep path from model-gateway.

5. Owner-only personal connections: today's emailsync feeds personal mailboxes into an org-wide inbox (F11b, OD11). Wide-research children get research tools only (C8), so they cannot touch personal connections.

6. Hosted browser only: E uses Verevon's own Chromium, never the user's Chrome. E9 shows the disclosure. E12 is deferred.

7. I6's KB indexing may use a paid embedding provider: ask first. H9 never uses Azure Document Intelligence for tests. I23 step 2 waits for a user-bound reviewer route.

8. Agents do not modify security settings of the running stack. Every policy grant (D1, D10, F7, G8, E11, C9) is prepared in source and applied by the owner through the Control-plane launcher.

9. Review-lever rule: nothing here changes source review, batches or thinking budgets.

   - I15 needs an A/B on the saved matrix if it changes effort or latency.

   - I9 changes governed artifact prompts: rerun the four journeys.

   - A1 keeps thinking_budget_tokens at 0.

CROSS-CUTTING RISKS:

- 300 s bearer: it bounds A (270 s deadline), E (240 s clamp), C, and B's executing runs. Plan runs need 4+3n rounds at 10 to 40 s each, so even a 3-step plan overruns the 240 s clamp on Terra. A mis-sized proof will look like a failure.

- Quota: one Codex account hit its cap on 2026-09-29. There is no preflight meter on the subscription route, because cost-core counts it as 0. The proof ladder is about 150 calls.

- Emulated tool JSON: nested arrays in plan.propose and plan.step_status can fail to parse on Terra. Measure the first-attempt parse rate in A6 and B11. Terra may also under-elect tools (tool_choice auto); add a required first-round tool_choice behind a flag if live runs show zero tool calls.

- Rollout order: an old execution-core ignores the new proto fields and would run the loop on model-router (paid). Enable the switch only after the new execution-core image is confirmed running. inference-core's 'agentic' capability follows the same variable. An A-side route echo from execution-core would catch a mis-ordered rollout.

- Shared dirty tree: many files carry other sessions' uncommitted edits. Never stash or revert. Regenerate pb files with tooling. Scan for NUL bytes after any reboot. The session cwd is not a git repo; the real checkout is C:/dev/CoresSystem (verify .git, beware OneDrive near-duplicates).

- Build only through the launcher. Frontend rebuilds restart the container serving live source (OD20).

- Capability gate: finding F3 (name-only HIGH capability makes every execute_provider_action ask) is from reading code, not observed. F13 settles it. cap.tool.provider.read and execute are source_only in the promotion ledger.

- Unverified assumptions to confirm early:

  - user tokens carry orchestration:write (B1 seed);

  - Session Core accepts a child-run bearer (C1 probe);

  - ExecuteStep accepts steps on a completed run (G9 spike; run the no-model spike in Wave 3 to de-risk Wave 5);

  - Graph and Google behaviours in F2, F3 and G7.

- J: the design returned no tasks. Re-run it. Its owner-only list (external audit, pentest, counsel review, Postmark account, cross-tenant spec, captions) is unsequenced here. The Postmark self-forward mail-to-task Phase 1 from the 2026-09-28 owner rulings needs a Wave 1 or 2 slot once J is designed.

## 9. Findings made while planning

- **The learning review sends runs to a paid model.** capability-core's background learning review runs on this stack: 173 review events in the container's log, 4 completed. inference-core logged `claude-sonnet-4-5` via `azure-anthropic` for those, at the same time as the 2026-09-30 gates were running live Terra turns. It is task A0 and is fixed first. The permanent rule (skip any thread that ran on a user-owned subscription route) is an owner decision (OD4).
