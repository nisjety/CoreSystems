## 5. Recommended

Recommended items are worth building but are not the first bets. Most sit on top of a Priority item (the sandbox computer, the capability gate, the browser run) or restate a slice of an existing Verevon plan. Verification changed several of them: some baselines were richer than first described, some efforts moved, and several bundles should be split into independent slices. Each card states the verified gap with code citations and the adjusted approach.

<a id="F-t3-024"></a>
### F-t3-024 · Computer use inside the agent's own sandbox desktop (consent tiers only if user-desktop control is ever unparked)
*Verevon today: Absent · Effort: XL · Plane: Model Plane (execution-core computer tool, guest agent); Endpoint Plane only for phase 2 · Wave: W4 · Confidence: Medium*

**What and why.** Add a computer tool whose Model-proposed actions (screenshot plus accessibility tree, click, type, scroll) are executed by a guest agent on a desktop inside the sandbox VM, with a receipt per action. "Controls a computer" is part of the vision; doing it in the agent's own VM respects the owner's decision not to drive users' desktops.

**Reference practice.** Desktop computer use ships in 8 of 12 references; consent tiers are a differentiator (2 of 12). Manus returns an accessibility tree with short handles and an optional screenshot with a capture id, acts by handle or coordinates plus capture id, and refuses ambiguous targets [SRC]. Claude adds per-app consent tiers, a deny list, a global lock and restoration at turn end [SRC].

**Verevon today and approach.** No desktop tool exists: Do mode offers only browser.observe/browser.act and code_interpreter (apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs:4190-4222) on a one-shot bubblewrap child (apps/Model Plane/rust/services/execution-core/src/sandbox.rs:255-310); the Endpoint Plane agent is Windows-only and untargeted. Order: (1) S, now: write the typed connector, then Quarry browser, then screen hierarchy into the Do prompt and capability policy; (2) after [F-t3-009](#F-t3-009) and [F-t3-035](#F-t3-035), extend that typed executor protocol with a desktop surface modelled on Quarry's snapshot-scoped refs and step receipts; (3) keep the coordinate fallback off until an ADR.

**Dependencies and risks.** Nearly all cost sits in [F-t3-009](#F-t3-009); user-desktop control stays parked; on-screen prompt injection and vision cost.

<a id="F-t3-025"></a>
### F-t3-025 · Open the typed action registry to the agent: classify every action, admit reads first, mint per-run scoped tokens
*Verevon today: Partial · Effort: M · Plane: Frontend Plane (registry) with the Model Plane capability gate and each owning plane · Wave: W2 · Confidence: High*

**What and why.** Classify every typed action by model access (none, read, write-with-approval) and risk, admit reads per owning plane through the capability gate, then writes under exact-effect approval, with per-run scoped authority. This is the core of the AI-first thesis: the agent uses the same actions and guards as the UI.

**Reference practice.** 4 of 12 references expose their own product to the agent. Claude desktop ships built-in MCP (Model Context Protocol) servers for sessions, sidebar and settings, and each Preferences key declares whether the model may read or change it [SRC]. T3 Code issues a per-session scoped bearer stored only as a hash [SRC].

**Verevon today and approach.** ActionDescriptor already carries risk, requiresApproval and reversible (apps/Frontend Plane/verevonv3/src/shared/actions/types.ts:15-24), but isModelExecutableAction admits 8 of 168 actions (apps/Frontend Plane/verevonv3/src/shared/actions/model-eligibility.ts:126-140). Control already signs per-run, per-action authority, yet runActionTarget accepts only tickets.create (apps/Control Plane/user-core/internal/spaces/run_action_decision.go:340-347). Generalise run-action-v1 and model-action-view-v1 into a per-action table keyed by the catalog schema hash; Control mints the authority, the BFF keeps minting only per-audience user bearers. Add a model-access class and exempt reason to ActionDescriptor and turn the catalog-manifest allowlist test into the CI ratchet, following plan milestones M2 and M3.

**Dependencies and risks.** [F-t3-003](#F-t3-003), [F-t3-006](#F-t3-006); the decision needs a non-ZDR run and Space evidence; each admission is its own M of owner-plane work.

<a id="F-t3-026"></a>
### F-t3-026 · Human takeover and hand-back from live views: agent-requested takeover for logins, 2FA and CAPTCHAs, then point-and-comment steering
*Verevon today: Partial · Effort: M · Plane: Model Plane (pause and resume) with Ingestion Plane (Quarry control mode) and Frontend Plane (takeover card, live view) · Wave: W3 · Confidence: Medium*

**What and why.** Let the agent ask for help when a browser run hits a login, two-factor prompt or CAPTCHA, hand the hosted browser to the user, and resume with the user's note and a fresh snapshot; human-typed input never reaches the model or replays. A second track lets the user steer by clicking an element or drawing a region.

**Reference practice.** 5 of 12 references hand control to the human and back. Manus shows agent-suggested takeover cards (view, take over, pause, exit takeover) and a hand-back message [DOC]. Cursor's design mode lets the user click elements or draw a region in the in-app browser and comment [SRC]; point-and-comment is a differentiator (2 of 12).

**Verevon today and approach.** Manual takeover exists: Quarry owns the control mode (apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/step_receipts.rs:55-95) and the BFF returns 409 to agent actions while a human holds control (apps/Frontend Plane/verevonv3/apps/gateway/src/domains/browser.rs:690-715). ChatLiveRunPanel mounts BrowserChrome with pause and resume but passes no onBrowserControlMode, so the toggle is disabled (apps/Frontend Plane/verevonv3/src/features/dashboard/home/BrowserChrome.tsx:621-628), and Quarry's ChallengeSignal is not consumed. Approach: (1) wire takeover and hand-back into ChatLiveRunPanel, closing plan E10; (2) add a human_assist kind on the existing durable approval gate, triggered by ChallengeSignal or a planner action, as a proposal Quarry accepts, with a longer wait budget; (3) point-and-comment later on the [F-t3-031](#F-t3-031) annotation part. Drop synthetic-input arbitration; Quarry's single control-mode state already covers it.

**Dependencies and risks.** [F-t3-008](#F-t3-008), [F-t3-002](#F-t3-002); any login carry-over grant needs an expiry.

<a id="F-t3-027"></a>
### F-t3-027 · Deep research v2: clarifying questions and an editable plan, mid-run steering, a gap-driven second pass with a claim ledger, and knowledge plus connector sources in one cited report
*Verevon today: Partial · Effort: XL · Plane: Model Plane · Wave: W3 · Confidence: High*

**What and why.** Split deep research into an editable plan step, a second pass that targets gaps and conflicts through a claim-to-source ledger, and one report that cites web, knowledge and connector sources. Plan transparency and gap-driven passes make research trustworthy at depth.

**Reference practice.** Reference evidence is documentation only. ChatGPT deep research moves through clarify, plan proposed, user edit, running and an incrementally streamed report, and a changed site list mid-run re-plans the rest [DOC]. Codex's deep-research skill loops plan, broad read, claim ledger, gap queries and a diminishing-returns stop [DOC]. Plan editing was recorded for 3 of 11 sources with research records, second passes for 5 of 11 [DOC].

**Verevon today and approach.** run_deep_research (apps/Model Plane/rust/services/model-gateway/src/deep_research.rs:2413-2470) is one in-stream, web-only pass: up to 9 sub-queries shown as a checklist, about 16 pages read, citations before synthesis, a mandatory could-not-verify section. There is no clarification, plan approval, ledger or second pass, and sse.rs:945-985 rejects knowledge_only scope with deep research. The pipeline dies with its SSE stream, so no gate can wait inside it. Ship three slices: a plan turn returning an editable sub-query node, then a run turn re-clamped server-side; an in-process second pass driven by the existing coverage shortfall inside the wall clock; knowledge and connector sources through existing viewer-ACL retrieval under retrieval_scope. Demote mid-run steering: queued_input only lands at tool-round boundaries.

**Dependencies and risks.** [F-t3-054](#F-t3-054), [F-t3-010](#F-t3-010); refused under ZDR, so no ledger persists there; extra passes raise synthesis cost.

<a id="F-t3-028"></a>
### F-t3-028 · Live run-cost meter with pre-run estimate, cost by token category, quota display and allowance warnings
*Verevon today: Partial · Effort: L · Plane: Model Plane (cost-core, model-gateway events) with Control Plane (quotas) and Frontend Plane · Wave: W2 · Confidence: High*

**What and why.** Emit cumulative usage progress after every provider call, show a live meter in the Work panel and Run Console, warn at 80 percent of the run or org ceiling, and later show a labelled estimate on the plan card. Verevon's honesty contract (cost null when unknown, basis marks subscription use) is extended, not replaced.

**Reference practice.** A live meter and estimate is a differentiator (2 of 12); an allowance page is table-stakes (7 of 12). Manus records credits per reply, states roughly how many credits a long operation will consume, keeps a credit ledger and pre-checks balance per metered tool [SRC]. Antigravity shows cost by input, cache read, output, thinking and tools per turn and subagent [SRC].

**Verevon today and approach.** ChatEvent::Usage fires once at turn end without cache fields (apps/Model Plane/rust/services/model-gateway/src/sse_events.rs:268-286), although the client reads cache_read_tokens (apps/Frontend Plane/verevonv3/src/shared/api/chat-client.ts:335-350) and the Prompt cache metric appears to stay at zero [INF]. Already present: the cost-core ledger with a per-run aggregate, org allowance page, quotas with reset period, and a silent 80 percent budget posture that routes one tier cheaper (apps/Model Plane/rust/services/inference-core/src/provider/routing_policy.rs:126-134). Start from plan I16 (UsageProgress frames plus warning), add cache fields to the Usage frame, and surface the silent downgrade in the same warning. Drop disjoint reasoning tokens (providers count them inside output); defer attribution, thread totals and the estimate. On ZDR turns compute the meter in-stream, since zdr ledger envelopes are suppressed.

**Dependencies and risks.** [F-t3-022](#F-t3-022), [F-t3-042](#F-t3-042); per-user allowance needs Control Plane billing support; estimates stay labelled.

<a id="F-t3-029"></a>
### F-t3-029 · Canonical reasoning-effort ladder mapped per model to each provider wire format
*Verevon today: Partial · Effort: M · Plane: Model Plane · Wave: W1 · Confidence: High*

**What and why.** Define one canonical effort scale and one resolve step in inference-core that every adapter calls: clamp to the nearest weaker level the model supports, then map to the provider vocabulary. The Budget and Balance defaults are mostly OpenAI-family models, and that route ignores the user's depth choice.

**Reference practice.** 6 of 12 references implement a ladder. Hermes keeps an EFFORT_LADDER with per-generation wire vocabularies and a clamp that picks the nearest weaker supported level instead of sending a value that returns 400, caching capability probes per model and base URL [SRC]. Codex declares a per-model ladder with descriptions, DeepSeek refuses unsupported values before dispatch, T3 Code renders controls from descriptors [SRC].

**Verevon today and approach.** Effort is a three-value profile (apps/Model Plane/rust/services/model-gateway/src/thinking.rs:1-80) that becomes an Anthropic thinking budget or a Codex low or high effort only. The OpenAI and Azure builder sends no reasoning effort even for gpt-5 and o-series ids (apps/Model Plane/rust/services/inference-core/src/provider/openai.rs:430-455), and the Do loop hard-sets thinking_budget_tokens to 0 (apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs:1334-1340). Start with the smallest correct slice: derive OpenAI and Azure reasoning_effort from the existing budget as codex_reasoning_effort does, for gpt-5 and o-series ids only. The six-level ladder needs an additive InferRequest proto field and composer levels, so it is step two. Keep the per-model table in inference-core; effort applies only to the final answer inference, not tool rounds.

**Dependencies and risks.** Effort changes cost, so defaults tie to budget posture; a per-role cap is a Control-owned entitlement; extending effort to Do runs is a product decision.

<a id="F-t3-030"></a>
### F-t3-030 · Workspace file tools for Do mode: versioned read, grep, glob and list, plus governed edit and write on one small Rust edit engine
*Verevon today: Absent · Effort: M · Plane: Model Plane (execution-core tools, capability-core ids) · Wave: W2 · Confidence: High*

**What and why.** Add bounded read tools over the hydrated Space workspace (ranged read with line numbers and a content hash, ripgrep-shaped grep, glob, list) and governed edit and write tools on one Rust edit engine with unique-match replace and a stale-read guard. Today agents change workspace files only by writing a whole program through code_interpreter.

**Reference practice.** Every coding-capable reference ships an edit tool; detailed records exist for 3 of the 9 references with coding records. Cursor's StrReplace uses a matching ladder, a structured result and a per-file lock; its grep maps onto bundled ripgrep with context lines and a truncated flag and refuses an unbounded broad glob [SRC]. DeepSeek adds a stale-read version guard with a remedy message [SRC].

**Verevon today and approach.** No read, grep, glob, edit or write tool exists in either catalogue (apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs:4090,4190-4222). Changes made through code_interpreter are diffed against a CAS (content-addressed storage) baseline and merged by sandbox-manager Promote with a per-path base_hash compare-and-swap (apps/Model Plane/go/services/sandbox-manager/internal/workspace/store.go:100-175). Implement reads first inside execution-core over the hydrated root, keep diff_and_upload and Promote as the only durable path, and derive the per-file stale token from the CAS baseline hash so both guards agree. Add the new ids explicitly to permission::is_risky_tool (apps/Model Plane/rust/services/execution-core/src/permission/mod.rs:229-275, a substring match fs.edit would miss) and to trusted_capability_id. Fix the redaction hash-domain issue first, since diff_and_upload hashes redacted bytes.

**Dependencies and risks.** [F-t3-003](#F-t3-003); durable only for Space-scoped runs until [F-t3-009](#F-t3-009)'s thread workspace; ZDR runs write nothing durable.

<a id="F-t3-031"></a>
### F-t3-031 · Artifact canvas: server-side versions with restore, user editing, selection comments and selection-scoped AI edits, annotation, then collaborative documents
*Verevon today: Partial · Effort: XL · Plane: Model Plane (version store) with Frontend Plane (editor), Application Plane (realtime) and Data Plane v2 (promotion) · Wave: W3 · Confidence: High*

**What and why.** Let users edit, restore and point at parts of an artifact in the Output panel, then add selection comments and annotation, and later collaborative documents. Canvas-style editing is how claude.ai, ChatGPT and Manus let users finish work with the agent.

**Reference practice.** Versioned artifacts with restore are common (4 of 12); selection comments, annotation and collaborative documents are differentiators (2 of 12 each). Manus keeps a server-side version list with restore and editor attribution (AI, owner, collaborator) [SRC]. T3 Code serialises a review comment with path, range, text and diff excerpt that round-trips into a comment card [SRC]. Claude desktop ships a Rust-to-WASM yrs CRDT (conflict-free replicated data type) document engine [SRC].

**Verevon today and approach.** The gateway already owns the version counter and current text and serialises run-versus-run commits with lock_commit and unchanged_from (apps/Model Plane/rust/services/model-gateway/src/artifacts.rs:137-232). There is no revision store with its own ids, no route to read, write or restore, no editor attribution, and the panel's version stepper is read-only (apps/Frontend Plane/verevonv3/src/features/chat/components/ChatArtifactPanel.tsx:195-345); no selection handling exists in verevonv3/src. Phase 1 is plan I17: client-side highlight-to-edit through the normal model revision path and restore by re-emitting an old revision as a new version; OD19 rules out a server range-patch API. A later user-save RPC goes through the existing lock_commit path with an editor attribute. Annotation (M) and yrs documents (XL) are separate later bets.

**Dependencies and risks.** [F-t3-012](#F-t3-012), [F-t3-052](#F-t3-052); ZDR threads persist no artifacts; save to knowledge must create a Data Plane document under the user's token.

<a id="F-t3-032"></a>
### F-t3-032 · Org agent policy registry: named agent modes, setting locks, a lockdown profile, extension allow-lists, frozen tool snapshots, durable hooks and one Customize hub
*Verevon today: Partial · Effort: XL · Plane: Control Plane (policy registry) with Model Plane (capability-core enforcement, model-gateway mode clamp) and Frontend Plane (hub) · Wave: W2 · Confidence: High*

**What and why.** Add named agent modes clamped to min(org cap, user choice), a signed tighten-only org policy registry with locks and a lockdown profile, frozen tool snapshots after admin approval, durable hooks that reach Do runs, and one Customize hub. The enforcement core exists; hook rules are in memory and admins cannot set modes or locks.

**Reference practice.** Named modes ship in 8 of 12 references and admin registries in 7 of 12; lockdown is a differentiator (3 of 12). Codex uses a typed mode vocabulary with per-effect approval config and a full-access confirmation that lists risks [SRC]. Claude keeps a schema-as-source registry with a fail-closed value per key, tighten-only remote config and 33 hook events [SRC]. ChatGPT freezes a custom app's tool list after admin approval [DOC].

**Verevon today and approach.** Only PermissionMode Auto/Ask/Deny (apps/Model Plane/rust/services/execution-core/src/permission/mod.rs:6-58; from_wire maps unknown strings to Auto), a server-floored posture and an AutonomyRung grant exist, with no org cap. The hook engine is real but HookRegistry is in-memory and the Do loop passes an empty hook_context (apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs:1900-1930); a durable hook_configs table exists in session-core (migrations/0004_tasks_cron_hooks_skills_memory.sql:160-185) but only erasure touches it. Admin sections for MCP servers, plugins, skills and org instructions already exist. Split: plan B4 (S) passes hook_context into Do runs; durable hooks plus admin API (M); AgentMode mapped onto PermissionMode plus AutonomyRung (M); Control-signed registry, locks and lockdown (L); snapshots pinning name, description and schema hashes, plus the hub (M).

**Dependencies and risks.** [F-t3-003](#F-t3-003), [F-t3-006](#F-t3-006); owner-plane effects are never auto-approved; new modes must not ride from_wire until it fails closed.

<a id="F-t3-033"></a>
### F-t3-033 · Content-safety hardening: an injection normaliser, enforcing semantic screening for web and browser content, one policy check on every memory and skill write, and safe links
*Verevon today: Partial · Effort: L · Plane: Model Plane (model-gateway moderation, session-core memory) · Wave: W2 · Confidence: High*

**What and why.** Add a Unicode normaliser before the injection scan, make semantic screening enforcing for external web and browser content after a false-positive evaluation, run one deterministic policy on every memory and skill write, and add safe-link handling. Exposure grows as browser and connector reads arrive.

**Reference practice.** Injection defences are table-stakes (8 of 12). Perplexity documents the most layered stack: intent anchoring, a screening classifier, containment and runtime monitoring [DOC]. Claude's scrubber normalises text, strips the U+E0000 tag block and zero-width characters, and neutralises lookalike angle brackets and reserved envelope tags [SRC]. Hermes stages memory and skill writes and scans installs against a trust matrix [SRC].

**Verevon today and approach.** Shipped: trust-class provenance, a fail-closed full-payload scan, hash-only security events and a 20 percent sampled semantic shadow pass (apps/Model Plane/rust/services/model-gateway/src/semantic_screening.rs:1-60,93-116) wired only into the chat tool loop. scan_injection substring-matches 16 phrases with no normalisation, the Do-mode seam gets only the marker scan (apps/Model Plane/rust/services/execution-core/src/runtime_loop/mod.rs:795-820), IndexMemory checks nothing beyond non-empty content (apps/Model Plane/rust/services/session-core/src/memory_grpc.rs:444-500), and learned skills are upserted unscanned. Order: (1) a shared normaliser crate used by both scanner copies, with a test corpus; (2) a deterministic memory policy at IndexMemory and extractor output with a typed refusal; (3) enforcing posture for ExternalWeb and Browser after the evaluation, chat first, Do mode needing a new pass; (4) a chat link scheme allowlist reusing ooxml.ts isSafeExportHref; (5) skill-write scan before promotion is enabled.

**Dependencies and risks.** Evaluation set from shadow telemetry; semantic screening may only tighten posture and is skipped under ZDR; never log flagged content.

<a id="F-t3-034"></a>
### F-t3-034 · Trajectory ledger with exact model-visible request capture and timing waterfall
*Verevon today: Partial · Effort: XL · Plane: Model Plane (session-core, model-gateway, execution-core) and Frontend Plane (Trace tab) · Wave: W3 · Confidence: High*

**What and why.** Add a second view of every run: a virtualised ledger grouped by turn, step, request and tool call with a per-record inspector, an Inspect action on each tool card, request snapshots written at the model call site, and a timing waterfall drawn only from recorded timestamps. Nobody can see today what the model was actually sent, which also blocks replay tests.

**Reference practice.** A ledger is a differentiator (2 of 12); request capture is common (4 of 12). DeepSeek keeps the invariant that anything model-visible is logged: one append-only per-session log, a request header event appended only when it changes, a diffable system-prompt event, auxiliary requests logged before dispatch, and a waterfall from recorded timestamps only [SRC]. Antigravity adds queue and approval time to timing; Cursor exposes copyable request ids [SRC].

**Verevon today and approach.** The Trace tab is a flat projection capped at 5,000 events (apps/Frontend Plane/verevonv3/src/features/chat/components/ChatPanels.tsx:1400-1466); per-turn timing is content-free and not persisted per step (apps/Model Plane/rust/services/model-gateway/src/turn_timing.rs:1-40). No snapshot of the exact system prompt, tool catalogue or parameters exists, and the gateway trajectory store is an in-memory ring buffer for training export. Order by risk: (1) group existing run events by turn, step and action_id with an inspector drawer and copy trace id, needing no new storage; (2) persist step timing as content-free events; (3) request capture last, short-retention, tenant-keyed, hashes only under ZDR, wired into thread delete, Space deletion and GDPR purge.

**Dependencies and risks.** [F-t3-049](#F-t3-049), ZDR resolver; owner-only access, no ACL widening; which platform prompt text is shown is a product decision.

<a id="F-t3-035"></a>
### F-t3-035 · Live computer panel with typed viewers (browser, read-only terminal, files, later desktop) and model-facing PTY exec sessions
*Verevon today: Partial · Effort: L · Plane: Frontend Plane (panel) with Model Plane (execution-core process host) · Wave: W4 · Confidence: Medium*

**What and why.** Turn ChatLiveRunPanel into a computer panel that picks a viewer by tool kind: browser frames, a read-only terminal fed by redacted process output, file and diff viewers, later a desktop. Give the model a PTY (pseudo-terminal) option with long output spilled to workspace files. This is how a user watches the agent work.

**Reference practice.** A computer panel and terminal panes each ship in 4 of 12 references; output offload is a differentiator (2 of 12). Manus switches viewer by tool kind and keeps a picture-in-picture mini view that follows the user [SRC]. Codex's exec_command takes tty, yield time and max output tokens and returns a session id with partial output for write_stdin polling [SRC]. Cursor stores each terminal as a file and offloads big outputs to disk [SRC].

**Verevon today and approach.** ChatLiveRunPanel mounts only BrowserChrome and a screenshot rail (apps/Frontend Plane/verevonv3/src/features/chat/components/ChatLiveRunPanel.tsx:590-625), but a ToolIntent registry already renders terminal, search, read and diff presentations (apps/Frontend Plane/verevonv3/src/shared/chat-nodes/tool-presentation.ts:1-80). The process_* family exists with a redacting batcher, durable registry and cursor pull (apps/Model Plane/rust/services/execution-core/src/process_tools.rs:1-70), but uses pipe stdio, needs a Space lease and a feature flag, and has no human read path for output. Key the viewer registry on ToolIntent with browser, process and file kinds; add an optional tty flag to process_start and reuse process_read instead of a parallel exec contract; add output spill in the same family. The unbuilt human output read path is a prerequisite for the terminal viewer.

**Dependencies and risks.** [F-t3-009](#F-t3-009), [F-t3-008](#F-t3-008); desktop viewer blocked on [F-t3-024](#F-t3-024); registry output must fall under ZDR and erasure; no interactive user shell.
