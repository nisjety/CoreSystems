<a id="F-t3-048"></a>
### F-t3-048 · Readable long runs: transcript density levels, settled-turn folding, a docked plan strip and tool narration
*Verevon today: Partial · Effort: M · Plane: Frontend Plane with Model Plane (optional render hints, narration metadata) · Wave: W3 · Confidence: Medium*

**What and why.** Add a per-user transcript density preference, fold settled turns behind a "Worked for X" row, dock a Step N of M plan strip in the chat transcript, and narrate tool rows with per-action sentences. Long agent transcripts are where readability decides whether users trust the agent.

**Reference practice.** View modes appear in 4 of 12 references; folding and the plan dock in 3 of 12 each; narration in 1 of 12. T3 Code derives one fold row per settled turn, measures duration from the triggering user message and never folds a running turn [SRC]. DeepSeek applies four work-detail levels at turn, group and row level [SRC]; Codex keeps about 900 localisable narration templates keyed connector.tool.state [SRC].

**Verevon today and approach.** The StepsPill, collapsible reasoning trace, RunPlanPanel and the "Steg N av M" label (apps/Frontend Plane/verevonv3/src/features/agents/components/AgentRunConsole.tsx:2335-2355) ship, and Spaces has a verb/object/outcome grammar (apps/Frontend Plane/verevonv3/src/features/spaces/lib/activity-grammar.ts:1-120). Nothing in the SPA reads the persisted compactMode (apps/Frontend Plane/verevonv3/apps/gateway/src/onboarding/theme/appearance.rs:13-22), there is no folding or in-transcript plan strip, and tool-presentation.ts:25-165 derives only an intent icon and counts. Derive everything from existing events with no new persistence; lift the Spaces grammar to shared with a tool-call adapter; reuse the compactMode slot for density. Folding must never hide approval, plan-approval, question or effect-receipt nodes. Find-in-collapsed content and reasoning summaries become separate items.

**Dependencies and risks.** [F-t3-021](#F-t3-021), [F-t3-066](#F-t3-066); any recap model call must not run on ZDR turns.

<a id="F-t3-049"></a>
### F-t3-049 · Typed run lifecycle events and status cues: retry, compaction, context-change rows, thread status, connector incidents
*Verevon today: Partial · Effort: L · Plane: Model Plane (model-gateway, execution-core, capability-core) with Frontend Plane · Wave: W2 · Confidence: Medium*

**What and why.** Add the missing lifecycle events to ChatEvent: retry_scheduled, a compaction marker with counts only, and context-change or notice rows (history shed on overflow, skill guidance truncated, trigger kind). Derive one thread status with fixed precedence and an unseen-completion state. Today retries, history shedding and compaction are silent, so users see a stall with no explanation.

**Reference practice.** Retries with typed end reasons, compaction markers and status pills each appear in 1 of 12 references; message source and connector incidents in 2 of 12. DeepSeek appends a retry event before waiting, emits a CompactionItem with token counts and types every message source [SRC]. T3 Code resolves status with precedence, rollup and unseen completion [SRC]; Claude desktop keeps the last 10 incidents per built-in MCP server with a redacted stderr tail [SRC].

**Verevon today and approach.** ChatEvent has done, Stopped and Error frames (apps/Model Plane/rust/services/model-gateway/src/sse_events.rs:142-180,340-395) but no retry, compaction or context-change variant; context overflow sheds history log-only (sse.rs:2905-2990) and tool retries run unseen (apps/Model Plane/rust/services/execution-core/src/runtime_loop/retry.rs:1-60). Do not add turn_end: normalise the three terminal frames client-side. Start with the silent history shedding. Retry visibility needs a field on the inference-core response (proto change), the AG-UI agent-run vocabulary must be covered too, trigger chips need run origin on the run record, and connector incidents split off into capability-core with redaction.

**Dependencies and risks.** SSE taxonomy gate; compaction markers carry counts only and never the summary under ZDR.

<a id="F-t3-050"></a>
### F-t3-050 · Scrubbable browser-run replay with per-action briefs and a walkthrough clip in the run proof
*Verevon today: Partial · Effort: M · Plane: Frontend Plane (scrubber) with Ingestion Plane (Quarry recording) and Model Plane (proof bundle) · Wave: W4 · Confidence: High*

**What and why.** Add playback and a time slider to the existing browser step timeline, show a one-line brief per action in the Run Console and chat rail, and attach a walkthrough clip reference to the proof bundle. It is the best answer to "what did the agent do on that site", and the event data already exists.

**Reference practice.** Scrubbable replay is unique to Manus (1 of 12 references); recordings as a habit appear in 2 of 12. T3 Code builds a timeline slider from toolUsed events and records browser frames at 12 fps [SRC]; Cursor makes recordings routine [SRC].

**Verevon today and approach.** Model Plane emits dispatched (with reason), decided and observation events linked by action_id with screenshot_ref (apps/Model Plane/rust/services/execution-core/src/browser_events.rs:100-215), and BrowserChrome has a step timeline with rationale and screenshot (apps/Frontend Plane/verevonv3/src/features/dashboard/home/BrowserChrome.tsx:457-515). Missing: a slider or auto-play, the brief in the Run Console, and any clip: Quarry captures one screencast frame at a time (apps/Ingestion Plane/Quarry-v2/crates/quarry-browser/src/chromiumoxide.rs:1700-1760) and RunProofBundle has no browser section (apps/Model Plane/proto/model_plane/v1/proof_bundle.proto:30-52). Extend BrowserChrome, not a second timeline. Assemble a filmstrip from the per-step screenshots Quarry already stores before building continuous video [INF]; the scrubber alone is S and a quick win. The bundle carries a reference, never bytes.

**Dependencies and risks.** [F-t3-008](#F-t3-008), [F-t3-034](#F-t3-034); no clip exists under ZDR by construction; a retention and redaction policy must precede shipping.

<a id="F-t3-051"></a>
### F-t3-051 · Images as a governed agent tool: typed generation, explicit provider choice, an edit endpoint with a mark tool, and C2PA provenance
*Verevon today: Partial · Effort: XL · Plane: Model Plane (model-gateway, inference-core) with Frontend Plane (mark tool) and Control Plane (signing certificate) · Wave: W4 · Confidence: Medium*

**What and why.** Expose generate_image to the chat tool loop and the Do loop with a typed result, a per-run semaphore, cost accounting and no silent fallback; later add an edit endpoint with a mark tool, C2PA (Coalition for Content Provenance and Authenticity) signing and a second provider. Without this, image mode is a dead end and reports cannot include generated visuals.

**Reference practice.** Generation is table-stakes (4 of 11 references), editing common (4 of 11), C2PA unique (1 of 11). Codex returns a typed result and asks before falling back to a weaker model [SRC]; Cursor caps concurrent image calls per run [SRC]. Manus draws marks as masks for edit instructions [SRC, DOC], and Claude desktop ships a Rust/WASM C2PA module [SRC].

**Verevon today and approach.** Generation is a request flag that takes over the turn via image_gen_stream with hard-coded 1024x1024 (apps/Model Plane/rust/services/model-gateway/src/sse.rs:1415-1440,5945-6130); inference-core has one provider family that silently walks the chain (apps/Model Plane/rust/services/inference-core/src/provider/vision.rs:1-60,130-160); cost-core has no image pricing and Model Plane has no durable binary artifact store. The revised prompt is already surfaced as the artifact title. Sequence: (1) agent tool with semaphore, priced through cost-core and gated by ZDR and capability policy (M); (2) explicit provider selection surfacing model_used; (3) decide the durable home for binary artifacts; (4) C2PA signing where bytes are produced; (5) mark tool and edit; (6) Gemini adapter.

**Dependencies and risks.** [F-t3-017](#F-t3-017), [F-t3-041](#F-t3-041); generate_image is refused on ZDR turns today; split the agent tool from edit and provenance.

<a id="F-t3-052"></a>
### F-t3-052 · Composer: atomic typed context tokens, per-thread drafts and thread references
*Verevon today: Partial · Effort: L · Plane: Frontend Plane (editor, drafts) with Model Plane (reference resolution, thread tools) · Wave: W3 · Confidence: Medium*

**What and why.** Make references to Data Plane documents, past threads, Spaces and skills explicit typed tokens in the send payload, persist a text draft per chat thread, and let the agent browse referenced threads through read-only outline, search and read tools. Typed references suit Verevon's authority model, and losing a long prompt on reload is a top frustration.

**Reference practice.** A typed composer is table-stakes (6 of 12 references); drafts and thread references appear in 2 of 12 each. T3 Code uses decorator nodes per token type and versioned localStorage drafts with byte budgets [SRC]. Manus lets a task reference up to 20 task ids checked lazily at read time, with outline, search and read tools instead of injected content [SRC, DOC].

**Verevon today and approach.** Slash commands with a server catalogue and an entity overlay ship, but the composer is a textarea plus overlay and @ is reserved for Space agents (apps/Frontend Plane/verevonv3/src/features/dashboard/home/DashboardComposer.tsx:3874-3885); only Home persists a draft (apps/Frontend Plane/verevonv3/src/shared/session/home-draft-storage.ts:1-78) while the chat page keeps in-memory state (ChatPage.tsx:925-945); no thread references exist. Send a structured context_refs array beside plain text, rendered by the existing overlay, before committing to a rich-text editor; generalise home-draft-storage to a per-thread key with its retention gate and sign-out purge (S, quick win); build thread references on [F-t3-012](#F-t3-012). Large-file handoff belongs to [F-t3-017](#F-t3-017) and targets imports-core.

**Dependencies and risks.** Use a trigger other than @; refuse ZDR threads for references; referenced thread content is untrusted data; Space access never widens an owner-plane ACL.

<a id="F-t3-053"></a>
### F-t3-053 · Claim-level citations: bind chips by server number, a cited-passage viewer and a provenance stamp on research artifacts
*Verevon today: Partial · Effort: M · Plane: Model Plane (anchors) with Frontend Plane (chips, viewer) and Data Plane v2 (passage reads) · Wave: W3 · Confidence: Medium*

**What and why.** Bind citation chips by the number the server already emits, open the cited passage from a chip, and stamp a provenance record on research artifacts. Verevon's sources panel is already ahead of most references; the missing link is from a claim to the exact passage.

**Reference practice.** Passage-level source viewing appears in 2 of 11 references, file chips that open at a line in 2 of 12. ChatGPT opens connected files beside the chat and Perplexity offers a per-claim Check Sources view; both rest on public docs only [DOC]. T3 Code's file chips open at a line [SRC]; Claude Science describes versioned artifacts with provenance records [DOC].

**Verevon today and approach.** The server numbers citations and emits claim_id and offsets (apps/Model Plane/rust/services/model-gateway/src/sse_events.rs:533-546), but the client drops number (apps/Frontend Plane/verevonv3/src/shared/api/chat-client.ts:953-969) and binds [n] by array position (chat-media-markdown.tsx:1250-1268). KB grounding already carries document_id, chunk_index and fact text (retrieval.rs:84-146) while the Sources tab shows only a count (ChatPanels.tsx:476). Export is shipped client-side and leaves this item. Sequence: add number to the client model (planned I6, S); build the passage viewer for KB sources from facts already on the wire; add a web locator from the Quarry artifact id for non-ZDR turns. Defer web authority labels (no domain-class data exists) and the paid per-claim support check.

**Dependencies and risks.** [F-t3-027](#F-t3-027); viewers open only documents the viewer may read through the Data Plane read API; no stored web page exists on ZDR turns.

<a id="F-t3-054"></a>
### F-t3-054 · Web reading upgrade: chunked long-page reader and a batched, reference-id web tool
*Verevon today: Absent · Effort: M · Plane: Model Plane · Wave: W3 · Confidence: High*

**What and why.** Stop cutting fetched pages at 4,000 characters: return an outline, the first chunk and a reference id, and add read_page and find_in_page over the full text with no new egress. Let web_search take up to four queries and return one merged, de-duplicated list with stable reference ids. B2B users research long primary sources, and reference ids make the cited page the page the model opened.

**Reference practice.** Batched operations and chunked fetch each appear in 2 of 11 references. DeepSeek stores long pages, returns an outline plus chunk 1 with view_chunk(n), and runs 1 to 4 de-duplicated queries, interleaved by rank and capped at 8 [SRC]. Codex uses typed operations with reference ids so open, click and find act on an id, not a URL [SRC].

**Verevon today and approach.** fetch_url truncates at MAX_FETCH_CHARS = 4,000 (apps/Model Plane/rust/services/model-gateway/src/tool_loop.rs:124-126,2987-3055), web_search takes one query (tool_loop.rs:2874-2950), no read_page, find_in_page or queries[] exists, and deep research shares the same cap (deep_research.rs:165-176). Back read_page and find_in_page with the Quarry artifact id the gateway already resolves (quarry.rs:606-650,780-840), with a turn-scoped in-memory buffer only under ZDR [INF]; rank with grounding::select_passages. Parallel web_search calls already run concurrently via join_all (tool_loop.rs:7204-7210), so queries[] saves tokens and rounds, not latency. Decide whether the execution-core Do loop (8,000-character cap, web_tools.rs:38) is in scope.

**Dependencies and risks.** SSRF vetting stays in Quarry; every chunk stays untrusted and screened; wider fan-out must respect allow_paid_providers and org quota.

<a id="F-t3-055"></a>
### F-t3-055 · Web access governance and provider transparency: org web policy, domain rules, cached-only mode and "Searched via" disclosure
*Verevon today: Partial · Effort: L · Plane: Control Plane (policy authority) with Model Plane and Quarry (enforcement) · Wave: W3 · Confidence: High*

**What and why.** Add an org web policy (web on or off, allowed and blocked domains, paid providers, cached-only mode, provider pin) stored in org-core, enforced in both web tool loops and re-checked by Quarry, and make routing visible with a provider-and-reason field and a "Searched via X" line. Enterprise admins need to restrict where agents read and see where queries go.

**Reference practice.** Governance appears in 3 of 11 references, the provider seam and data-handling disclosure in 1 of 11 each. Claude desktop has an org capability switch, per-domain fetch consent with an admin-skippable preflight and egress allow-lists [SRC]. Codex offers disabled, cached or live search modes [SRC]; DeepSeek names the provider with explicit selection states [SRC].

**Verevon today and approach.** A per-turn web toggle and RetrievalScope are enforced in the inline loop (apps/Model Plane/rust/services/model-gateway/src/tool_loop.rs:1420-1480), paid providers are gated by tier and ZDR (quarry.rs:180-275), and Quarry's host list is a security feed, not org-configurable (apps/Ingestion Plane/Quarry-v2/crates/quarry-security/src/preflight.rs:1-60). No org policy, consent, cached-only mode, router decision or disclosure exists, and execution-core web_tools.rs:240-262 sends only query, limit and zdr. Read the policy through a narrow fail-closed service endpoint like the retention posture, not a bearer claim; pass a policy revision to Quarry on every call. Ship "Searched via" first (S to M). Verify the provider table against smart_router.rs before publishing it.

**Dependencies and risks.** [F-t3-032](#F-t3-032); cached-only mode must add no egress; per-domain prompts only on Do runs.

<a id="F-t3-056"></a>
### F-t3-056 · Connectors: in-chat suggestion cards with resumable connect, and editable write confirmation with user-visible policy levels
*Verevon today: Partial · Effort: L · Plane: Model Plane · Wave: W3 · Confidence: High*

**What and why.** Let the model return a provider key and reason that the frontend renders as an opt-in card, with a connect intent that resumes the turn after OAuth; and show an editable preview card for mail drafts and calendar events before a write, with user-visible per-tool policy levels. Editable cards are the difference between a trusted and a feared "send on my behalf".

**Reference practice.** Suggestions appear in 4 of 11 references, write policy and cards in 2 of 11. Claude desktop gives the model suggest_connectors, keeps the connect intent across the login redirect and uses a four-level tool policy [SRC]. Manus emits editable email-draft and calendar-event cards and applies exactly the confirmed payload [SRC]. Claude.ai never ranks by payment [DOC].

**Verevon today and approach.** Read/write classification already exists twice (apps/Model Plane/rust/services/execution-core/src/integration_tools.rs:60-71,747-769; integration-corev2 Capability.Direction) and Do-mode writes pause for durable approval with a frozen, fingerprint-bound descriptor (runtime_loop/agent.rs:4594-4650). Missing: an editable preview (the card offers only approve or reject, ChatMessages.tsx:1890-1925), any suggestion tool or connect-intent record, and user-visible policy levels. Reuse the connect-session routes the gateway already proxies; do not add an integration-corev2 client to model-gateway. Unify the execution-core catalogue with Direction, build cards on planned G8 and G13, and map user levels onto the existing Ask decision and autonomy rung.

**Dependencies and risks.** [F-t3-010](#F-t3-010), [F-t3-003](#F-t3-003); an edited payload must produce a new descriptor and fingerprint; approval continuations are refused under ZDR; writes default to always-ask.

<a id="F-t3-057"></a>
### F-t3-057 · Custom connectors: probe-and-test add flow, secret-reference MCP auth, and a REST/API-key connector kind behind an egress proxy
*Verevon today: Partial · Effort: L · Plane: Model Plane with Ingestion Plane (integration-corev2 executor and egress proxy) · Wave: W4 · Confidence: High*

**What and why.** Add a probe that returns an MCP (Model Context Protocol) server's tools and auth requirement without saving, a tool picker, a secret-reference step for static-token servers, and a REST/API-key connector kind executed by a server-side proxy that injects vaulted secrets. This lets a business plug in its own ERP, CRM and internal APIs while keeping keys out of model context.

**Reference practice.** Custom connectors appear in 4 of 12 references, host-brokered secrets in 3 of 12. Manus validates the server, auto-detects OAuth, inspects tools before save and supports API connectors with secrets [SRC]. Claude desktop injects vaulted credentials with per-operation confirm rules and a network allow-list [SRC].

**Verevon today and approach.** Register, share, OAuth and a one-call connect exist, and connect auto-discovers tools/list (apps/Model Plane/rust/services/model-gateway/src/http_routes.rs:2771-2870), but there is no probe or tool picker (McpServersSection.tsx:150-215), and validate_mcp_server rejects any raw token, so static bearer servers cannot be registered at all (runtime_registries.rs:410-470). Implement the probe over http_list_tools and discover_protected_resource. Put the REST executor and proxy in integration-corev2, reusing its SSRF client, allowlist and vault. The sandbox proxy-only namespace is a separate item: AllowDomains is advisory and code_interpreter forces Disabled (apps/Model Plane/rust/services/execution-core/src/code_interpreter.rs:575-583).

**Dependencies and risks.** [F-t3-003](#F-t3-003), [F-t3-043](#F-t3-043); registration is refused under ZDR; loose allow-lists make the proxy an exfiltration path; secrets never reach logs.

<a id="F-t3-058"></a>
### F-t3-058 · One versioned lifecycle for authored behaviours (skills, slash templates, presets) with a /learn command and curation
*Verevon today: Partial · Effort: XL · Plane: Model Plane (capability-core) with Frontend Plane (libraries) · Wave: W3 · Confidence: Medium*

**What and why.** Give skills immutable versions with digests pinned on runs, draft and publish, Space and org libraries, import, authored presets, a linter on every upsert path, and a /learn command and Save-as-skill action through the proposal path. Without versions a skill edit silently changes every run that matches it.

**Reference practice.** Skill distribution appears in 6 of 12 references, presets and agent-assisted authoring in 4 of 12, /learn in 3 of 12, authoring doctrine only in Hermes. Manus keeps personal, team, project and official libraries with versions, restore and project lock [SRC]. Hermes /learn takes a directory, URL, notes or "what we just did" and writes a lean index with per-chapter references [SRC]. Claude desktop refuses an org skill whose SHA-256 differs [SRC].

**Verevon today and approach.** agent_skills is unique on (org, name) and UpsertAgentSkill overwrites in place (apps/Model Plane/rust/services/session-core/src/grpc.rs:5075-5115); the learning loop persists candidates enabled with no proposal step (apps/Model Plane/go/services/capability-core/internal/skillsink/sink.go:41-62); a versioned skill_packages schema exists unused (capability-core migrations/0003_capabilities_registry.up.sql:134-182); presets are hard-coded (preset-agents.ts:1-40). Choose one owner and table first. Ship the thin layer (doctrine, linter, /learn, Save-as-skill, S to M) because it needs no schema change, then versions with ContentHash as digest and the quarantine re-keyed by version, then libraries and import. Make the background loop propose rather than enable.

**Dependencies and risks.** [F-t3-014](#F-t3-014), [F-t3-007](#F-t3-007); /learn must reuse the sessionreview rule (refuse unless zdr:false); Space libraries need a schema change; org promotion stays an admin decision.

<a id="F-t3-059"></a>
### F-t3-059 · Plugin manifest loader with supply-chain pins, cross-vendor adapters and an org-curated catalogue
*Verevon today: Stub · Effort: L · Plane: Model Plane (capability-core) with Control Plane (entitlement and audit) · Wave: W4 · Confidence: High*

**What and why.** Build the loader that expands a plugin into child capabilities (skills, remote HTTP MCP servers, slash commands first), pins every source to a commit SHA or archive digest with a kill list, lists every component at install, and starts with an org-curated catalogue. A plugin is how a vertical solution reaches an org.

**Reference practice.** Plugin packages are table-stakes (7 of 12 references), marketplaces and cross-vendor compatibility common (4 of 12), supply-chain safety a differentiator (3 of 12). Codex accepts the open agent-plugins.org 1.0 manifest with foreign-namespace adapters and atomic upgrade with rollback [SRC]. Hermes pins about 570 catalogue entries to a 40-hex commit, clones every entry in CI and keeps a removed.yaml kill list [SRC].

**Verevon today and approach.** capability-core stores plugin_packages with manifest_json, risk_level and rollout_state but no digest, source or signature column (apps/Model Plane/go/services/capability-core/migrations/0003_capabilities_registry.up.sql:240-259), and the handler says registration is inert until a host loads the manifest, which nothing does (internal/api/plugins.go:13-22). Keep catalogue and loader in capability-core and use Control Plane for entitlement and audit only. Expand skills, remote MCP (through normalizeMCPRegistration) and commands into existing registries and refuse stdio, bin and script components with a report. Add digest, source and kill-list columns; defer foreign-layout adapters until the internal manifest is stable. Sequence after [F-t3-058](#F-t3-058).

**Dependencies and risks.** [F-t3-003](#F-t3-003), [F-t3-032](#F-t3-032); install is admin-only with consent to the declared component set; hooks may only narrow authority; ZDR and Space authority apply to every child.
