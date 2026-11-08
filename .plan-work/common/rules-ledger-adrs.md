# Decisions, rules and ADR digest (Model Plane / CoreSystem)

Produced 2026-10-14, read-only. Worktree HEAD afb83928 (clean) and live tree C:/dev/CoresSystem were both checked.

Aliases used below (all paths absolute in the source tree):
- LEDGER = C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Model Plane/docs/decisions/ledger.md
- MASTER = C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/master-ownership-matrix.md
- CAPM = C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Model Plane/docs/capability-ownership-matrix.md
- ROOTCLAUDE = C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/CLAUDE.md
- MPCLAUDE = C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/apps/Model Plane/CLAUDE.md
- CHATGAP = C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/plans/system-audits/CHAT_GAP_CLOSURE_PLAN_2026-10-01.md (live; identical in worktree)
- OWNRUL = C:/dev/CoresSystem/apps/Frontend Plane/verevonv3/apps/verevon-web/plans/system-audits/program-2026-10-03/owner-rulings-and-defaults.md (live; worktree copy differs only in CRLF)
- AUDIT = C:/Users/ImaFernandesDaCosta/AppData/Roaming/Claude/scratch-workspaces/5e23d905-dd91-4518-8ff4-252efcbe0520/b5ff8b3a-437c-4499-9a7f-469de994f319/scratch-2026-10-12-69a4ee/03-model-plane-chat-and-agents-audit.md

Checks: LEDGER, MASTER and CAPM are byte-identical in both trees (cmp). Line numbers below are the same in both trees. Worktree-only vs live-only: none found for these files. Live-only untracked plan files (BUILD_PASS_*, PRODUCT_CARD_FILMS_AUDIT_2026-10-11.md) were only grepped.

Tag key: `[RELEVANT:x]` = the entry decides or changes something on topic x. `[mention:x]` = topic appears in the body only.

---

## 1. Model Plane decision ledger (LEDGER)

Format: date | title | state | heading line | tags. Source of state: `**State:**` line when present (32 entries); otherwise the state in the heading (25); otherwise implied from the body and marked "implied" (12); the remaining 5 are sub-entries (3 corrections, 1 open decision, 1 archived). Corrections have no state. File header (L1-21) defines states: proposed, implemented, deferred, rejected, archived. Entries are newest first. 69 `##` entries and 5 `###` sub-entries (74 lines).

- 2026-10-06 | Service principals reach the retrieval engine by a route allowlist, not membership (bounded exception to rule 9) | proposed (built in working tree, not deployed) | L25 | [mention:ZDR]
- 2026-10-06 | inference-core: typed posture refusals, one metering decorator, generated status golden | proposed (built and tested; flips at deploy) | L146 | [RELEVANT:ZDR]
- 2026-10-03 | Own-connection ZDR: signed, acknowledged, per-run waiver bound to one connection | proposed (body: designed, not built) | L186 | [RELEVANT:ZDR] [RELEVANT:scheduled] (never scheduled, unattended, browser, speech, background) [RELEVANT:routing/subscription] [mention:approvals] [mention:memory/skills] [mention:browser-grants]
- 2026-10-03 | The organisation's ZDR switch is gated on an attested provider | proposed (body: built and running; flip pending) | L345 | [RELEVANT:ZDR] [RELEVANT:entitlements]
- 2026-10-03 | Contextual retrieval stays on: bounded inline generation plus database-truth reconciler | proposed (nothing deployed until flip) | L390 | [RELEVANT:ZDR] (skipped_zdr) [mention:cache/prompt]
- 2026-10-03 | Tenant retention posture: background work follows the organisation's zero-retention setting | proposed (Auth Core and org-service halves in images since 2026-10-04) | L602 | [RELEVANT:ZDR] [RELEVANT:routing/subscription] (background never uses a user's subscription or key) [RELEVANT:entitlements]
- 2026-10-03 | Background provider metering: usage events from Data Plane to cost-core | proposed (cost-core ingest not built; forwarder dark) | L729 | [mention:ZDR] [mention:routing/fallback]
- 2026-10-03 | Graph extraction: per-chunk state machine, loses its direct-Azure path | proposed (nothing built) | L853 | [RELEVANT:ZDR] (skipped_zdr) [mention:routing]
- 2026-10-03 | Web-search rerank: Data Plane owns reranking of caller-supplied candidates (Quarry option B) | proposed (nothing built or deployed until flip) | L921 | [RELEVANT:ZDR] [RELEVANT:routing/subscription] (reranking never on a subscription route)
- 2026-09-03 | Quarry-v2 web search: ZDR did not gate Brave/Serper | implemented | L1175 | [RELEVANT:ZDR]
- 2026-08-28 | Data Plane retrieval: what to send when nothing declares a sovereignty posture | implemented | L1242 | [RELEVANT:ZDR]
- 2026-08-28 | Data Plane suggested_next_tools: always-advertised tools, not dynamic injection | implemented | L1315 | [mention:ZDR] (zdr_actions_applied)
- 2026-08-22 | Sandbox-backend interface: defer a SandboxBackend trait | rejected (for now; revisit when a second real sandbox backend is actually being built) | L1358 | [RELEVANT:SandboxBackend] [mention:memory/skills] (cites memory-adapter precedent)
- 2026-05-30 | Memory-adapter registry: defer until a 2nd backend exists | deferred (revisit when a second real memory backend is being built) | L1390 | [RELEVANT:memory/skills]
- 2026-08-14 | HARN-1/2: unify the two tool-dispatch loops | rejected, on measurement (write-up: apps/Model Plane/docs/postmortem/0001-harn-1-2-tool-dispatch-unification.md) | L1410 | [RELEVANT:two-loop]
- 2026-08-22 | Semantic-memory erasure: one confirmation standard, not two | implemented | L1429 | [RELEVANT:memory/skills]
- 2026-08-22 | Skill guidance gets a size budget, not just a count cap | implemented | L1470 | [RELEVANT:memory/skills] [mention:two-loop] (duplicated across both loops) [mention:cache/prompt]
- 2026-08-22 | Context inspector is a read-only view, with no delegated Data Plane credential | implemented | L1501 | [RELEVANT:cache/prompt] (assembled context window)
- 2026-08-22 | Browser screenshots come from Quarry-v2, and artifacts get an authoritative content type | implemented | L1526 | [RELEVANT:browser-grants] [mention:MCP] (rejected service-token-for-MCP fix) [mention:ZDR]
- 2026-08-22 | The diff tool intent is detected from output, not from a tool name | implemented | L1563 | [RELEVANT:MCP] (third-party MCP tools)
- 2026-08-22 | The browser egress proof belongs in quarry-runtime, and it was hiding ~1000 tests | implemented | L1590 | [RELEVANT:browser-grants] (egress and approved hostnames)
- 2026-08-24 | Mid-run user input arrives as a pause, not an interruption (the model classifies queued input) | implied implemented (no State line; text: refusal paths state their reason) | L1639 | [RELEVANT:queued-input] [mention:approvals]
- 2026-08-24 | Compaction is recoverable, and recovery is need-driven | implied implemented (no State line; edits the prompt, not the thread) | L1674 | [RELEVANT:cache/prompt]
- 2026-08-24 | Memory provenance has three states, and the third must show | implied implemented (no State line; later qualified at L2402, L2728) | L1694 | [RELEVANT:memory/skills]
- 2026-08-24 | A subagent's conclusion lives on the child run, and the parent needs consent to read it | implied implemented (no State line) | L1713 | [RELEVANT:approvals] [mention:ZDR] (zero-retention parent)
- 2026-08-24 | The autonomy ladder: a graded grant, per call, with a stated reason | implied implemented (no State line; C7 grant was inert until L2659) | L1759 | [RELEVANT:approvals]
- 2026-08-24 | Fork semantics for subagents: REJECTED, as the reference rejected it | rejected (title) | L1798 | none
- 2026-08-24 | Per-run child ceiling instead of worktree isolation | implied implemented (no State line; MAX_TOTAL_CHILDREN = 8) | L1821 | none
- 2026-08-24 | A replayed delegation resumes; it never re-runs | implied implemented (no State line) | L1847 | [RELEVANT:approvals] (resume from runs.final_output)
- 2026-08-24 | Compaction and its recoveries belong to both loops | implied implemented (no State line) | L1874 | [RELEVANT:two-loop] [mention:memory/skills] (reattach_skill)
- 2026-08-25 | Needle 2 / Cactus: ideas yes, engine no | implied rejected for the engine, ideas-only (no State line) | L1908 | [mention:MCP] (mcp_call) [mention:routing] [mention:memory/skills]
- 2026-08-25 | Needle Phase 0 ran; Phase 1 cancelled | rejected (title "cancelled"; "Do not build a Needle serving path") | L1990 | [mention:two-loop] (tool catalogue sizes)
- 2026-08-25 | strict: true on function tools: deferred, it disables parallel tool calls | deferred (title) | L2038 | [RELEVANT:two-loop] (parallel dispatch in both loops, body L2050)
- 2026-08-25 | A malformed tool schema degrades loudly, in both providers | implied implemented (no State line) | L2078 | [mention:two-loop] (tool-dispatch path)
- 2026-08-25 | Tool arguments are validated before dispatch, in both loops | implied implemented (no State line; tests cited L2131) | L2103 | [RELEVANT:two-loop] [mention:MCP] (mcp_call arm)
- 2026-08-25 | Our tool-dispatch path measured against the Needle harness | no canonical state: "measured, one new defect found" (not fixed in this pass, L2181) | L2146 | [RELEVANT:two-loop]
- 2026-08-25 | Tool calling checked against published research | no canonical state: "two defects found, none fixed" (open) | L2183 | [mention:two-loop] [mention:approvals] (approval gating does not help)
- 2026-08-25 | Elicitation snippet for user-only tool arguments | implemented (title: measured both directions) | L2221 | [mention:two-loop] [mention:cache/prompt]
- 2026-08-25 | skill_budget_contract.rs source-text parser fooled by rustfmt | fixed (title) | L2261 | [RELEVANT:memory/skills] [mention:two-loop] (both loops must truncate skills identically)
- 2026-08-25 | Argument grounding (section 5) implemented in both loops | implemented (title) | L2284 | [RELEVANT:two-loop]
- 2026-08-25 | web.search/web.read were classified OrgInternal: injection-defense hole | fixed (title) | L2311 | [mention:two-loop] (trust class on both surfaces)
- 2026-08-25 | The grounding gate was inert in the chat loop | fixed (title; chat was measured at 100% fabrication before the fix) | L2336 | [RELEVANT:two-loop] [mention:cache/prompt] (compose_system_prompt not used by chat)
- 2026-08-25 | Adoption scorecard verified claim-by-claim | no canonical state: finding, 8 of 22 claims built but unreachable | L2371 | [RELEVANT:memory/skills] (save_memory and recall_memory advertised and dispatched but unreachable; provenance vacuous) [RELEVANT:queued-input] (queued-input fix client-side dead, L2395)
- 2026-08-26 | P0 critical-path list verified item-by-item | no canonical state: finding, 4 of 10 confirmed as stated | L2418 | [RELEVANT:MCP] (P0-8 MCP containment, L2464-2468) [RELEVANT:memory/skills] (P0-7) [mention:approvals] (approval continuation test) [mention:ZDR]
- 2026-08-26 | ### Correction to L2418: P0-10 coverage figures are reproducible, and both are wrong | correction (no state) | L2495 | none
- 2026-08-26 | Chat elicitation + the third shipping spelling | implemented, measured (title) | L2532 | [RELEVANT:two-loop] (chat vs agent loop) [mention:MCP] (mcp_oauth)
- 2026-08-26 | The 8 reachability defects fixed | implemented (title: all 8 closed; two were inert, corrected at L2659) | L2591 | [RELEVANT:memory/skills] (save_memory and recall_memory, H1) [RELEVANT:queued-input] (P3) [RELEVANT:approvals] (C7 autonomy grant)
- 2026-08-26 | ### Correction: two of the 8 "fixes" were inert (C7 grant no-op; memory attestation) | correction (no state) | L2659 | [RELEVANT:approvals] [RELEVANT:memory/skills]
- 2026-08-26 | Live proof against the running stack | no canonical state: 3 of 8 proven live | L2714 | [RELEVANT:memory/skills] (H1, D7) [RELEVANT:queued-input] (P3 needs a browser) [mention:approvals] [mention:browser-grants]
- 2026-08-26 | ### The remaining 4 need a credential decision, not more code | open (decision needed; no state) | L2770 | [mention:browser-grants]
- 2026-08-26 | ### The dev auth bypass does not work: a NEW defect | archived (superseded by the L2843 correction) | L2803 | none
- 2026-08-26 | ### CORRECTION: the dev auth bypass is NOT broken | correction (no state) | L2843 | none
- 2026-08-26 | The 12 incomplete parity rows, worked through | no canonical state: 4 stale, 1 red CI gate, 7 remain open | L2883 | [RELEVANT:memory/skills] (memory lifecycle surface) [mention:queued-input] (queued_input doc) [mention:routing] (multi-provider routing)
- 2026-08-26 | Chat elicitation weakness fixed: a dangling cross-reference | implemented (title: fixed) | L2948 | [RELEVANT:two-loop]
- 2026-08-26 | Memory lifecycle surface: the gap does not survive a call-site check | no canonical state: closed without build, one invariant pinned | L2995 | [RELEVANT:memory/skills]
- 2026-09-11 | Sandbox lease identity: a ninth delegated user bearer, not a service token or token exchange | decided (title; B.1 shipped, B.2 not started at entry; B.2 closed at L3194) | L3043 | [mention:ZDR] (zdr equality)
- 2026-09-11 | S3.3 step 3.5 slices (a)-(c) implemented | implemented (title) | L3136 | [mention:browser-grants] (execute_step_with_browser_grant) [mention:two-loop] (RunAgent no-tool slice)
- 2026-09-11 | S3.3 step 3.5 slice (d): release the lease at run end | implied implemented (no State line; "Closed out phase B.2") | L3194 | none
- 2026-09-12 | S3.3 step 3.5.C designed, plus a pre-existing authz bug | decided, not implemented at entry (later implemented at L3296 and L3333) | L3246 | none
- 2026-09-12 | S3.3 step 3.5.C.1-C.4 implemented; ActivateLease authz fix regression-locked | implemented (title) | L3296 | none
- 2026-09-12 | S3.3 step 3.5.C.5: code_interpreter uses a durable, hydrated workspace when Space-scoped | implemented (title) | L3333 | none
- 2026-09-12 | S3.3 step 4: PromoteWorkspace merges a run's overlay into its Space | implemented (title) | L3396 | none
- 2026-09-12 | S3.3 step 5: Data Plane v2 promotion client; S3.3 complete | implemented (title) | L3483 | [mention:ZDR] (own ZDR gate)
- 2026-09-13 | S4.2 durable process registry designed; S3.3 scope bug fixed | proposed (design doc); the bug fix is implemented (State L3542) | L3540 | [mention:SandboxBackend] (deferred trait trigger, L3577) [mention:entitlements] (process_registry_entitled) [mention:ZDR]
- 2026-09-13 | S4.2 step 1: durable process registry | implemented (State L3628) | L3626 | none
- 2026-09-13 | S4.2 step 2: process RPCs; the Space stops being a parameter | implemented (State L3702) | L3700 | [mention:entitlements]
- 2026-09-13 | S4.2 step 3: Control grants space:processes; processes claim validated | implemented (State L3788) | L3786 | [RELEVANT:entitlements] (deny-by-default column) [mention:scheduled] (ScheduleFireEntitled)
- 2026-09-13 | S4.2 step 4: process host in execution-core | implemented (State L3849) | L3847 | [mention:approvals] (direct HITL rejects an undurable approval pause)
- 2026-09-13 | S4.2 step 5: process_* tool family; Space check in execution-core | implemented (State L3955) | L3953 | [RELEVANT:entitlements] (entitled Space only) [mention:approvals] [mention:ZDR] (ZDR gate not done, L4035)
- 2026-09-13 | S4.2 step 6: human read of Space processes under the run ceiling | implemented (State L4057) | L4055 | none
- 2026-09-14 | S4.3 general Watch primitive designed; capability-core owns it | proposed (State L4176; nothing implemented) | L4174 | [RELEVANT:scheduled] (a Watch is structurally a Schedule, L4179) [mention:SandboxBackend] (precedent, L4205) [mention:memory/skills] (memory-adapter precedent)
- 2026-09-14 | S4.3 step 1: Watch core; a claim does not hold locks across the network | implemented (State L4286) | L4284 | [mention:scheduled] (cron analogy)
- 2026-09-14 | S4.3 step 2: process-output adapter; a ceiling only one reader could see | implemented (State L4389) | L4387 | none
- 2026-09-14 | S4.3 step 3: a watch can exist; authority check moved from read to disclosure | implemented (State L4496) | L4494 | [RELEVANT:entitlements] (no new entitlement, L4555) [mention:scheduled] (sweeper) [mention:approvals] [mention:ZDR] (tests, L4608)

### Topic index (primary entries; mentions in brackets)
- SandboxBackend trait: L1358 (rejected, revisit at second real backend). Mentions: L3540, L4174.
- Two-loop separation / tool-dispatch unification: L1410 (rejected), L1874, L2038, L2103, L2146, L2284, L2336, L2532, L2948. Mentions: L1470, L1990, L2078, L2183, L2221, L2261, L2311, L2883, L3136.
- Queued-input classification: L1639 (model classifies); L2371, L2591, L2714, L2883 (the queued-input fix and P3 status).
- Persisted message versions: NO ledger entry. The rejection is recorded in apps/Model Plane/docs/CHAT_RESUME_AND_VERSIONS_SPEC.md (status line L3; "Why the first design was rejected" L38 and L87; corrected client-only v1 L101; server persistence only later, L124).
- MCP: L1563, L2418. Mentions: L1526, L1908, L2103, L2532.
- Memory / learning / skills: L1390, L1429, L1470, L1694, L2261, L2371, L2418, L2591, L2659, L2714, L2883, L2995. Mentions: L186 (learning review skipped on waived runs), L1874, L1908, L1990, L4174.
- ZDR: L146, L186, L345, L390, L602, L853, L921, L1175, L1242. Mentions: L25, L729, L1315, L1526, L1713, L2418, L3043, L3483, L3540, L3953, L4494.
- Approvals / continuation: L1713, L1759, L1847, L2591, L2659. Mentions: L1639, L2183, L2418, L2714, L3847, L3953, L4494.
- Scheduled runs / draft-only: L186 (never scheduled or unattended under a waiver), L4174 (a Watch IS a Schedule). Mentions: L3786, L4284, L4494. The owner ruling of 2026-09-28 ("scheduled runs are draft-only", see section 4) is NOT recorded in the ledger.
- Browser grants: L1526, L1590. Mentions: L186, L2714, L2770, L3136.
- Cache / prompt order: L1501, L1674. Mentions: L390, L1470, L2221, L2336.
- Routing / subscription fallback: L186, L602, L921. Mentions: L729, L853, L1908, L2883.
- Entitlements: L345, L602, L3786, L3953, L4494. Mentions: L3540, L3700.

---

## 2. Ownership rules and rulings

### 2a. MASTER: section 3, non-negotiable cross-plane contracts (rules 1-18)

- R1 No direct database crossing; Model and Quarry never connect to Data Plane Postgres or Qdrant. L157
- R2 No independent embedding or reranking outside isolated eval labs. L158
- R3 No agent bypass of Quarry policy: Model proposes browser actions, Quarry executes or rejects. L159
- R4 ZDR propagates across planes; amended by R17. Quarry reads the run's zdr flag from the body for service callers and the zdr claim only as a floor. L160
- R5 GDPR policy metadata (purpose, lawful basis, retention, residency, privacy class, deletion scope) travels with data. L161
- R6 Knowledge assets (graphs, wiki, embeddings, chunks, source logs, contradiction indexes) are Data Plane-owned. L162
- R7 Reasoning (autoresearch, graph synthesis, wiki maintenance agents, browser planning) is Model Plane-owned. L163
- R8 UX (page and graph editing, CLI/TUI, IDE bridge, channel inbox, voice and canvas) is App/Shell-owned. L164
- R9 Identity and membership are Control Plane-owned. Headers, session hints and Application projections are not grants. Interactive users unchanged. Bounded exception by R18 for service principals. L165 (rule 9)
- R10 Application projections only narrow access; a denial must revoke the projection; an authority outage must never widen access. L166
- R11 Notification intake is Application Plane-owned (POST /api/v1/notification-requests). L167
- R12 Provider execution never invents approval authority. Conversation owns durable human intent; Ingestion Integration owns provider execution and the single-use receipt. An effectful call needs a tenant-bound service bearer plus a short-lived signature over the exact authorization, actor, tenant, effect, payload digest and idempotency key. Every other issuer, including Model Plane, fails closed. L168 (rule 12)
- R13 Core Infra is traffic-only; it shares no plane database, cache, broker, store, tenant scope, provider credential or user authority. L169
- R14 Support Plane is transport-only for remote sessions and grants nothing; no "start remote session" action contract until an owning core persists records; device control is per session and never inferred from view permission. L171
- R15 Rerank of caller-supplied candidates (including public web results) is Data Plane-owned (POST /v1/rerank on retrieval-engine-rs). Callers never rerank. Paid platform provider only, never a subscription or bring-your-own route. A restrictive ZDR mode or sovereign requirement returns an explicit skipped marker. Only a named service principal with the rerank scope may call. L173
- R16 Background content processing (contextual retrieval, graph extraction and community summaries, index-time embed and rerank) resolves the org's ZDR posture at service-token mint through Auth Core, fails closed (503 retention_posture_unavailable, defer), uses skipped_zdr markers, never lowers ZDR org-wide, never uses a user's subscription or BYO connection, meters every paid call as exact usage to cost-core, and keeps contextual retrieval on (kill switch is incident-only). Recorded gap: index-time text embedding, page-image embedding and rerank are not yet on the posture oracle. L175
- R17 A user's own connection may lower ZDR for ONE run only through a signed, acknowledged waiver bound to that connection. Auth Core is the sole issuer and re-reads posture, the connection capability record and the stored acknowledgement; it signs only for a service-attested BFF principal and stamps a connection-bound zdr_waiver claim. A client can never send zdr:false. Enforcers (model-gateway, inference-core, execution-core) honour only the named connection and never re-route. session-core writes an immutable zdr_waived receipt. A waived run filters restricted documents and skips content-derived background jobs that would send content to other processors. Never for Temporary chat, Space policy, scheduled, unattended, support, inbox, browser or speech work, background jobs, or an operator switch. No admin lock (owner ruling 2026-10-03). Order: enforcers ship before the issuer; kill switch ZDR_WAIVER_ENABLED default off. State written: "designed, not built". L177 (rule 17)
- R18 A first-party service principal is admitted to the Data Plane retrieval engine by a default-deny allowlist of (service, method, matched route pattern, scope), not by membership: a bounded exception to R9. Auth Core mints only for a registered principal and an allowed org. Contradictory identity gives 401. An x-org-id that differs is refused. The request context keeps only data:read and data:rerank; no per-user grants; the user-core facade is never asked; /v1/rerank refuses users before any membership lookup; gRPC refuses services outright. Other Data Plane services that admit service principals must meet the same terms; none audited yet. Not decided here: which scopes a principal holds (Control registry). L179 (rule 18)

Other MASTER text that governs ownership:
- L10-16 Application authority note (2026-07-13): Control is the source of truth for identity and exact organisation membership; Application may keep revocable projections, owns the notification API and delivery ledgers, and fails closed when Control is unavailable.
- L25-31 Learning extension (2026-09-24): learning-plane owns course definitions, assignments, attempts and progress; Data owns materials and retrieval; Model owns reasoning, speech execution and agent runs; Control owns identity, authorization, canonical audit and the planned WhisperFlow service API and processing policy; Application owns realtime projections and notifications; Support stays transport-only. ADR-001 at L34.
- L41 Owner question table: Model Plane owns planning, reasoning, agents, tool choice, memory or wiki writes and research loops. L42 App Shell owns human-facing UX. L43 Core Infra routes traffic only. L45 Go owns durable workflow, registry, policy, scheduling, grants, billing and operator control.
- L54 Browser actions: Quarry owns execution; Model is a consumer via the agent contract. L55 Browserbase profile and grant lifecycle sits in Go.
- L69-70 Embedding and contextual retrieval: Data Plane owner, metered under R16; Model routes only.
- L71-72 Retrieval and rerank: Quarry and Model are callers only; Data Plane owns rerank.
- L80-81 Model Plane owns routing decisions (OpenClaw), tools, toolsets, MCP and cron (Hermes); Data Plane keeps memory persistence only.
- L85-86 MCP and plugin registries: Model owns capability use and runtime policy; capability-core-go owns the registry.
- L87 Tasks and cron: Model owns agent tasks; the row still names "orchestrator/task-core-go" (stale, see 2d).
- L88-89 Cost and metering: cost-core-go is the ledger of record; Data Plane owns the usage event and its delivery (R16).
- L90 Own-connection ZDR row: Auth Core is the only issuer; integration-corev2 holds the capability record and acknowledgement ledger; Model Plane owns enforcement only.
- L121-134 Model Plane target services: session-core L126 (threads, runs, checkpoints, memory index, compaction); inference-core L127 (provider routing, prompt cache); execution-core L128 (tool loop, hooks, browser-agent protocol, secret scrub, subagents); orchestrator-core L129 (Temporal, autoresearch, wide research, tasks and cron, approvals, recovery); capability-core L130 (tools, skills, plugins, MCP, model routing, safety, memory adapters); browser-broker L131 (trusted browser grants); letta-bridge L132 (optional memory bridge, not source of truth); cost-core L133 (ledger, budgets).

### 2b. CAPM: rulings and numbered items (no numbered rules; rulings by section)

- L42 Layering rule: gateway runtime_registries, coordinator and approvals stores are ephemeral caches. They must hydrate from the durable owner and publish mutations. They are not systems of record.
- L52 Identity, org and billing: Control Plane (external).
- L53-56 Session-core: thread and message timeline (L53), run lifecycle (L54), plans, steps, todos, approvals and subagent lineage (L55; gateway caches must defer), checkpoints, context assembly and compaction (L56).
- L57 Memory index: session-core `agent_memory`; letta-bridge is a separate question (see 4.4).
- L58 Durable tasks and cron: task-core versus session-core tables; flagged RESOLVE.
- L59 Tools, commands, skills, models, plugins, MCP, routing and safety registries: capability-core; gateway registries are caches (CONSOLIDATE).
- L60 Policy, permissions and RBAC: capability-core (policy/engine.go); execution-core is enforcement.
- L63 Provider routing and inference: inference-core.
- L64 Secret scrub: execution-core (scrub.rs).
- L65 Sandbox lease lifecycle: sandbox-manager owns leases; no real isolation yet.
- L66 Real OS isolation (bwrap, Landlock, seccomp, egress): execution-core plus sandbox-manager; flagged GENUINE GAP at write time.
- L67 Background process rows: sandbox-manager (Postgres sandbox_processes), owner as of S4.2 (2026-09-13).
- L68 Background process lifecycle (the OS process): execution-core process_host.rs; never writes rows except through RPCs. Split is deliberate.
- L69 Watches: capability-core, ONE owner with TWO records on purpose (AUTO-2 fire-once trigger vs S4.3 cursor subscription).
- L70 Subagent spawn, message and coordinate: orchestrator-core owns lifecycle; execution-core executes; gateway TeamWorkerStore is a cache.
- L73 Browser grants: browser-broker (Go), in-memory plus Redis target.
- L74 Cost and usage ledger: cost-core.
- L76 Knowledge graph and wiki extraction: Data Plane.
- §4.1 L86 Orchestration ruling: session-core (durability) plus orchestrator-core (lifecycle); gateway stores stay read-through and write-through caches; "No new coordinator system." L92 approval id crux (client-supplied id); implemented as G8 at L146.
- §4.2 L101 Ruling: retire task-core's scheduler and store; durable cron = Temporal; durable task state = session-core; metadata = capability-core. L102 RETIRED 2026-05-30.
- §4.3 L106 Ruling: capability-core is the registry system of record; gateway registries become read-through caches; no second registry implementation. L107 emission complete; L149 gateway consumer built (removal-only scope).
- §4.4 L110 Ruling: session-core owns the canonical memory index; letta-bridge becomes one adapter behind a capability-core memory-adapters registry; not a parallel memory system. L111 DEFER: build the MemoryAdapter registry when a second real backend lands.
- §4.5 L114 Ruling: capability-core owns the skills registry; orchestrator-core SkillPromotionWorkflow writes into it; the learning loop is a producer of candidates, not a store. L144 clarifies that skill bodies live in session-core agent_skills and the capability-core registry is the catalog.
- §5 gap rows: G1 L124 OS isolation (execution-core, codex linux-sandbox, Apache); G2 L125 MCP transport client (capability-core; stdio implemented L142); G3 L126 sandbox-policy vocabulary (implemented L138); G4 L127 ProviderCapabilities (implemented L137); G5 L128 secret-scrub hardening (implemented L136); G6 L129 keyring (SKIPPED, L139); G7 L130 closed learning loop (built end to end in code, L143-144; live LLM and NATS trigger not verified); G8 L131 durability for gateway ephemeral stores (approval write-through implemented L146; MCP write-through H.1 L147; skills no runtime write path L150; plan-mode remaining).
- L141 Isolation-model ruling (2026-06-03): per-tool-call bwrap executor is canonical for the agent loop; sandbox-manager per-session leases are complementary; provisioning deferred until a per-session consumer exists.
- L179-187 Protocol map: durable cron and coordination = Temporal; async events = NATS JetStream mp.v1.*; GraphQL not used.
- L194-198 Phase H (harmonize, no new features): tasks overlap (1), gateway approvals and plan mode (2), registries (3), letta-bridge as adapter (4), reconcile docs (5). L201-208 Phase G gap fills (items 6-12).

### 2c. Ownership lines outside these two docs

- ROOTCLAUDE L23 Control Plane owns identity, users, orgs, billing, sessions, audit, quotas and entitlements.
- ROOTCLAUDE L27 Model Plane owns reasoning, sessions/runs, inference, execution loops, capabilities, sandboxes, browser grants and cost.
- ROOTCLAUDE L28 Application Plane owns collaborative or realtime workspace projections and notifications.
- ROOTCLAUDE L34 Model may propose browser actions; Quarry-v2 executes or rejects them. L35 ZDR must propagate through any content-persisting boundary.
- MPCLAUDE L24 The two tool-dispatch loops (model-gateway dispatch_tool and execution-core execute_step_inner) must stay separate; the refusal boundary is the authority boundary.
- MPCLAUDE L25 Do not build a registry or interface for one implementation; sandbox-backend trait and letta-bridge deferrals are the precedent.
- MPCLAUDE L26 Check the ledger before proposing merges or new registries. L27 One canonical owner per capability (points to CAPM).

### 2d. Conflicts and gaps found while digesting

1. Stale row: MASTER L87 still names "orchestrator/task-core-go", but CAPM L102 says task-core was retired on 2026-05-30.
2. Approvals split: MASTER L129 lists approvals under orchestrator-core only; CAPM L55 and L83-86 put the durable record in session-core and the lifecycle in orchestrator-core. Not reconciled in MASTER.
3. Entitlements: ROOTCLAUDE L23 assigns entitlements to Control. Neither matrix has an entitlement row. The audit leaves allowed_models ownership open (AUDIT L1690 recommends Control decides and Model enforces; L1737 open question 14).
4. Audit trail: Control owns canonical audit (MASTER L29, ROOTCLAUDE L23). Model Plane postmortem apps/Model Plane/docs/postmortem/0002-session-audit-trail-never-delivered.md exists (title only read; cited in PROGRAM_SUMMARY_AND_STATUS_2026-10-07.md section 10 item 2).
5. Policy vs entitlements: CAPM L60 puts policy and RBAC in capability-core; ROOTCLAUDE L23 puts quotas and entitlements in Control. Different scopes; no ruling joins them.
6. "Sessions": ROOTCLAUDE L23 gives "sessions" to Control; ROOTCLAUDE L27 and CAPM L53-54 give sessions and runs to Model Plane. The word is used for different things; not reconciled in the docs read.
7. AUDIT L1719 says subscription fallback "clashes with ownership-matrix rule 17". MASTER L177 limits ZDR-off to the one named connection. The clash is stated only in the audit card F-t3-072.

---

## 3. ADRs: location, numbering, convention

### 3a. ADR files found (tracked; worktree and live identical; eval-corpus copies excluded)

| Path | Title (as in file) | Status (as in file) | Date |
|---|---|---|---|
| apps/SPACE_AUTHORITY_ADR_2026-08-13.md | ADR-0001: Canonical Space identity and authority | accepted for implementation; release gated by verification (L4) | 2026-08-13 |
| apps/CROSS_SPACE_AGENT_REGISTRY_ADR_2026-08-19.md | ADR-0002: Cross-Space agent registry ownership | accepted for implementation (L4) | 2026-08-19 |
| apps/AUTHORED_INSTRUCTIONS_ADR_2026-08-19.md | ADR-0003: Authored instruction hierarchy (platform + org + Space, composed with agent persona) | implemented (2026-08-20) (L4-L6) | 2026-08-19 |
| apps/GRAPHITI_ADOPTION_ADR_2026-09-07.md | ADR-0004: Graphiti is not adopted; take validity intervals only | proposed; no implementation authorised (L4) | 2026-09-07 |
| apps/learning-plane/docs/ADR-001-learning-plane.md | ADR-001: A shared Learning Plane with Teachme as its first product | Accepted for foundation; production integration pending evidence (L3) | 2026-09-24 |
| docs/architecture/adr-agent-loop-and-tool-execution-boundary.md | ADR: the agent-loop and tool-execution boundary stays split (no number) | accepted, 2026-08-14 (L3) | 2026-08-14 |
| docs/architecture/adr-surface-neutral-ingress-delivery.md | ADR: surface-neutral ingress and delivery (no number) | accepted, 2026-08-13 (L3) | 2026-08-13 |
| docs/architecture/adr-provider-side-orchestration-privacy-tiers.md | ADR: provider-side orchestration is never the only path for a privacy-tier customer (no number) | accepted, 2026-08-24 (L3) | 2026-08-24 |
| docs/ARCHITECTURE_DECISION.md | Architecture Decision: Gateway Service + NATS Integration | no status field | 2026-02-01 |
| docs/ARCHITECTURE_DECISION_CROSS_PLANE_CONTRACTS.md | Architecture Decision: Cross-Plane Contracts | no status field | 2026-04-08 |

Related decision records that are not ADRs: apps/Model Plane/docs/decisions/ledger.md (states, section 1); apps/Model Plane/docs/postmortem/0001-harn-1-2-tool-dispatch-unification.md and 0002-session-audit-trail-never-delivered.md; apps/Model Plane/docs/CHAT_RESUME_AND_VERSIONS_SPEC.md (status SHIPPED 2026-08-01; records the rejected first designs); apps/Model Plane/docs/core-research/grpc-safe-rebuild-decision-2026-07-13.md and -07-16 ("Decision Record" titles).

ADR content, one line each:
- ADR-0001: Space is the canonical collaboration identity (SpaceRef = org_id, space_id, kind).
- ADR-0002 (L38): Application Plane owns the cross-Space agent registry as an org-scoped Convex query. (Task premise said "agent definitions"; the file's subject is the registry.)
- ADR-0003: platform, then org, then Space instructions, then agent persona, composed in model-gateway. Used by the audits as "ADR-0003 precedence (OD19)" and by CHATGAP L276 (I21 waits on ADR-0003 precedence sign-off).
- ADR-0004: Graphiti not adopted; take validity intervals only (Data Plane v2, graph-index-rs).

### 3b. Numbering and template (observed; no README, index or template file exists in the repo)

- Series A, apps/ root: file `<TOPIC>_ADR_<YYYY-MM-DD>.md`; heading `# ADR-NNNN: <title>`; sequence 0001 to 0004 shared across files; next number ADR-0005 if this series is followed (inferred, not written down). Header fields `**Date**`, `**Status**`, `**Deciders**` (optional `**Scope**`, `**Supersedes / extends**`). Sections `## Context`, `## Decision`, `## Alternatives considered` (`###` per option), `## Consequences` (`### Positive`, `### Negative`, `### Risks and release gates`).
- Series B, docs/architecture/: file `adr-<kebab-topic>.md`, no number; heading `# ADR: <title>`; `**Status:** accepted — <YYYY-MM-DD>`; sections `## Context`, `## Decision`, `## Consequences`, plus topic sections.
- Series C, learning-plane: `docs/ADR-001-<topic>.md`; `**Status:**`, `**Date:**`, `## Context`, `## Decision`, `## Options considered`, `## Trade-offs and consequences`, `## Action items`.
- Planned, not present: apps/Studio Plane/README.md L177 plans `docs/adr/` per plane for "plane-specific architectural decisions".
- Model Plane tradeoffs: append to apps/Model Plane/docs/decisions/ledger.md (newest first; states proposed, implemented, deferred, rejected, archived; supersede with a new entry, never rewrite; MPCLAUDE L26 requires checking it first). Deep root-cause write-ups: apps/Model Plane/docs/postmortem/NNNN-<slug>.md (next 0003).
- Recommendation: use Series A for cross-plane product decisions (the audits cite ADR-0001 to ADR-0003 by number); use the Model Plane ledger for Model-internal design tradeoffs. The repo does not state which series is canonical.

---

## 4. Open decisions (OD), owner rulings and D10 / F12 as cited by the audit

Definitions are in CHATGAP (section 7 "Owner decisions (33)", L415; OD1-OD20 at L417-L512; OD-J1 to OD-J13 at L517-L577 belong to the J workstream and are not listed). Status is quoted or summarised from the file. "No ruling recorded" means the plan gives a recommended default but no decision.

| ID | Defined at | One-line statement (as written) | Status as written | Audit cites |
|---|---|---|---|---|
| OD2 | CHATGAP L422 | Enable Do mode on the ChatGPT subscription route; may it run effects; should a test-only stub route exist. | Recommended (L424): OFF in code and compose, ON only on the proof stack. Section 0 L13: "whether Do on Terra stays on by default" is still open ("on in the dev stack through the launcher environment only"). Wave 6 L276: "after OD2 flips the Do default". Not flipped. | AUDIT L1569, L1735 |
| OD4 | CHATGAP L432 | May the background skill-learning review send a subscription-route run's transcript to a paid model? | Recommended (L434): Never; skip threads run on a user subscription route. Interim fix A0 built and deployed (L9). L716: "the permanent rule ... is an owner decision (OD4)". Open. | AUDIT L450 ("OD4 open") |
| OD6 | CHATGAP L442 | Plan-gate defaults: does approval pre-authorise effects; auto-start; how the plan continues after an approval pause; round budget; recital; hooks. | Recommended (L444): each effect still asks; manual "Fortsett planen" in v1. No ruling recorded. B7 (continue after approval, row L178) not in the built list (L9). Open. | AUDIT L358, L1728 |
| OD7 | CHATGAP L447 | Wide-research design: budget semantics, entry point, who may use it, org limit. | Recommended (L449): separate child budget; no confirm gate in v1. No ruling recorded. C-series not in the built list (L9). Open. | AUDIT L858, L1586, L1733 |
| OD8 | CHATGAP L452 | Run-length credential: allow a longer-lived delegated bearer (up to 1500 s, hard cap 1800 s) beyond 300 s. | Recommended (L454): yes for wide-research turns only, after a security review, and only if C10 or B11 show runs over 240 s. 300 s bearer still bounds A to E (L57, L365). Open. | AUDIT L870, L1687, L1726 |
| OD9 (for D10) | CHATGAP L457 | Scheduled runs: scheduled-lane policy, owner identity on the subscription route (D10), reads, cadence, notification. | Recommended (L459): D10 option (a), inference-core only, 120 s, adversarial security review before activation; v1 reads web only and stays draft-only. Open. | not cited by audit under this number |
| OD10 | CHATGAP L462 | May the agent read the run owner's mail, files and calendar without a per-call approval; ZDR; Ask-mode tools (F12). | Recommended (L464): autonomous own-data reads behind PROVIDER_READ_TOOL_ENABLED (default off); refuse ZDR; do not build F12. No ruling recorded. Open. | AUDIT L498 |
| OD11 | CHATGAP L467 | Shared inbox vs owner-only personal connections (emailsync feeds personal mailboxes into the org-wide inbox). | Recommended (L469): (b) visibility "personal" or "org" chosen at connect time; interim stop syncing personal connections into the shared inbox. L467: "contradicts the owner-only ruling"; still true at L670. Open. | AUDIT L498 |
| OD12 | CHATGAP L472 | Accounts and tenants for live proof of connectors, writes and consent. | Recommended (L474): dedicated Microsoft 365 developer tenant and dedicated Gmail; never production mailboxes. L11: connector reads "not proven live, needs the owner"; G12 publisher verification is an owner action. Open. | AUDIT L498, L1560 |
| OD13 | CHATGAP L477 | Hosted-browser run rules: domain scope and policy owner; write answer into thread; planner on subscription; Quarry bearer; planner default. | Recommended (L479): hard fence of start host plus at most 8 user domains; broker is policy authority; planner default verevon-balance; QUARRY_BROWSER_AGENT_LLM OFF until E5 passes. Owner ruling of 2026-09-28 (L68) settles only "Chrome control means the hosted governed browser". Policy owner not ruled. Open. | AUDIT L100, L466, L1686, L1725 |
| OD18 | CHATGAP L502 | Office and data output choices: PDF fonts, OCR engine, attachment caps, truncation, deck theme, binary staging, editable cell, H11 and H12. | Recommended (L504): standard fonts; local Tesseract; raise cap to 4 MB for xlsx, pptx and pdf after H1 zip guard; 60k-character limit stays. Plan records only "Needs from the owner: OD18" (L230, L257). No ruling found in plans. | AUDIT L562 and L586 call it "owner ruling OD18 (4 MB, 60,000 characters)"; AUDIT L1541, L1564 |
| OD19 | CHATGAP L507 | Chat-core rulings: share-link model, presence, duplex voice, copy to a Space, projects, rewind scope, reasoning default, meter ceiling, selection edit, slash-template authors. | Recommended (L509), with "ADR-0003 sign-off needed" for server-synced folders. "Needs from the owner: OD19" (L230, L278). I22 (L226) records two non-build rulings only. Open. | AUDIT L99, L530, L538, L1685, L1724 |
| D10 (plan row) | CHATGAP L183 | Unattended owner identity for the subscription route (decision-bound token exchange, active-connection resolution). Size L; depends on D1, D4; owner decision yes. | Not in built list (L9). Adversarial security-review gate (L341). L195 cites "OD5 (D10 go/no-go and security review)", which conflicts with OD5 = proof window (L437). J6 parked "only after D10" (L291). Not built, open. | AUDIT L1687 |
| F12 (plan row) | CHATGAP L268 | Optional: the same reads in plain Ask mode through model-gateway tools. Depends on F2, F3, F4, F6, F8. | DEFERRED to Wave 6 (L616). Wave 6 trigger (L276): "only if A reports Do cannot run on Terra". OD10 recommends not building it (L464). | AUDIT L1734 (open question 11) |
| Owner ruling 2026-09-28 | CHATGAP L68 | "Scheduled runs are draft-only, personal connections are owner-only, Chrome control means the hosted governed browser, mail-to-task is Postmark self-forward." | "Owner rulings of 2026-09-28 stand" (L68). L712: the Postmark mail-to-task Phase 1 needs a Wave 1 or 2 slot once J is designed; J returned empty (L19). | AUDIT L101, L604, L1687, L1727 |

Related owner rulings found (2026-10-03 and later), not cited by the audit under these labels:
- OWNRUL L7-12: rulings 1 to 6 (no shortcuts; CR not switched off; no subscription reranking; Quarry option B; ZDR never org-wide for own-connection waivers; trust claims re-verified after build).
- OWNRUL L16-20: defaults D-A (ZDR switch gated on attested provider), D-B (waived runs skip memory indexing, dreaming and learning review), D-C, D-D, D-E (background budget alert-only).
- BUILD_PASS_6_8_2026-10-13.md L3 cites "owner rulings of 2026-10-12" (per-route retention for Z-15, QB-T09 gate-and-grant, Z-41b). Not read in full.

### Discrepancies and namespace collisions
- OD namespace collision: program-2026-10-03/build-manifest.md uses the same labels for other questions: OD4 = dedicated Azure alias for background LLM (L635); OD7 = org_processing_policy operator-only (L637); OD10 = keep gpt-4o-mini (L639); OD8 = paid re-seed approval (L206, L527, L615, L2469); OD11 = contextual item (L625). Only the CHATGAP meanings match the audit (OD4 L450, OD7 L858, OD8 L870, OD10 and OD11 L498). Read OD numbers with the source file.
- D10 and OD numbering inside CHATGAP: L195 names OD5 for D10 go/no-go; OD5 is the Terra proof window (L437); the D10 decision is OD9 (L457-L459).
- The audit calls OD18 an "owner ruling" (AUDIT L562, L586). CHATGAP records only a recommended default (L504) and "Needs from the owner" (L230, L257). No ruling record found.
- OD19 is tied to ADR-0003 precedence (AUDIT L1534 "ADR-0003 precedence (OD19)"; AUDIT L1685 "instruction precedence ... (OD19)"; CHATGAP L509 "ADR-0003 sign-off needed" and L276 "I21: after ADR-0003 precedence sign-off").

---

## Limits
- Ledger states without a State line are implied from the body; they are marked "implied" above and should be confirmed by the author.
- No ledger entry covers persisted message versions, MCP, scheduled draft-only, or the 2026-09-28 owner ruling. Their decisions live in CHAT_RESUME_AND_VERSIONS_SPEC.md, the audit, and CHATGAP.
- Not read in full: the audit (only the decision, open-question and card sections cited above), BUILD_PASS_* files, PROGRAM_SUMMARY sections other than 9 and 10, the postmortems other than their titles.
- Status of a decision after 2026-10-02 (CHATGAP status date) is not determined from this read, except where noted.
