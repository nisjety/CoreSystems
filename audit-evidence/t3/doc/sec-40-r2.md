<a id="F-t3-036"></a>
### F-t3-036 · Wide research: user entry point, honest estimate and spend gate, per-item retry, replay after reload
*Verevon today: Partial · Effort: L · Plane: Model Plane · Wave: W3 · Confidence: High*

**What and why.** Give the existing research fan-out engine a product surface: a "Bred forskning" control with item-count presets, a start card stating items, rounds and minutes, a per-item grid with retry, a mid-run spend check, and a grid that survives reload. The engine is already paid for and serves Verevon's B2B list work (suppliers, competitors, leads).

**Reference practice.** Manus is the only source, 2 of 12 references (both Manus surfaces), so this is a differentiator [SRC][DOC]. A controller splits the task into independent items, shows a cost range and a start card, spawns up to K isolated sub-runs, streams per-item status into a grid with retry, and after the first N items offers Continue or Wrap up before merging; sub-agents never talk to each other [SRC].

**Verevon today and approach.** The engine runs N delegated Researcher children behind a semaphore (default 4, cap 16) at `apps/Model Plane/rust/services/execution-core/src/subagent/research_fan_out.rs:1-110`, and ChatWideResearchPanel shows live cards by batchId (`apps/Frontend Plane/verevonv3/src/features/chat/components/ChatWideResearchPanel.tsx:1-130`). Missing: composer entry point, estimate card, spend check, per-item retry and replay; execution-core has no cost-core client. Plan rows C3 to C10 and OD7 already own entry point, limits, replay and credentials; the delta here is the estimate card (RunDeadline in `run_budget.rs:1-30` plus observed per-item time, USD only on metered routes), the spend check, and retry that re-spawns one child with the full item text. Rebuild the grid from lineage edges; under Zero-Data-Retention (ZDR) it stays live-only. Do not use the Go WideResearchWorkflow: it hardcodes run ids and cannot fire from a scoped schedule (`workflow_dispatcher.go:215-260`).

**Dependencies and risks.** [F-t3-028](#F-t3-028), [F-t3-002](#F-t3-002), OD7; the org cap must clamp client-requested counts server-side, and the 300 s delegated bearer bounds scale until C9 or OD8.

<a id="F-t3-037"></a>
### F-t3-037 · Persistent goals with a token budget, a completion judge and auto-continuation
*Verevon today: Absent · Effort: XL · Plane: Model Plane · Wave: W3 · Confidence: High*

**What and why.** Add a durable Goal record {objective, success_criteria, token_budget, used, status} and a continuation driver that starts budgeted Do runs until an audited judge confirms the criteria or a budget or blocker stops it. Today a user cannot hand over a multi-step objective and come back to a result.

**Reference practice.** Goals appear in 6 of 12 references [SRC][DOC]. Codex persists a ThreadGoal with a token budget and an audited completion check; Hermes adds an auxiliary judge with quality gates; the DeepSeek harness shows a goal bar with compare-and-set edits [SRC]. When the run goes idle, the driver re-prompts with the goal state and a cheap judge checks completion with evidence; the worker never self-declares done [SRC].

**Verevon today and approach.** No Goal record, budget ledger, judge or driver exists. The nearest primitives are an in-memory TeamWorker without budget or judge (`apps/Model Plane/rust/services/model-gateway/src/coordinator.rs:1-80`) and the scheduled-step lane, one tool-less 1,024-token inference per step (`apps/Model Plane/rust/services/execution-core/src/grpc.rs:496-525`), which cannot continue a Do run. Build (1) a Goal record and ledger in session-core, non-ZDR only; (2) a continuation workflow modelled on ScheduledRunSupervision that takes fresh Control authorization per continuation and starts a governed RunAgent, which needs the unattended Do-run lane (plan D10, OD8) first; (3) a judge that prefers the proof bundle and postcondition results (`postcondition.rs:1-40`) with a rubric fallback through inference-core. Budget must come from cost-core run usage and is a no-op on the Subscription route. An attended first slice (L): a goal bar in the Agent Run Console that auto-continues while the session still holds delegated authority.

**Dependencies and risks.** [F-t3-002](#F-t3-002), [F-t3-001](#F-t3-001), [F-t3-018](#F-t3-018), OD8; never auto-continue across approval-required effects.

<a id="F-t3-038"></a>
### F-t3-038 · Deck pipeline with brand templates: tool-owned slide manifest, per-slide validation, present mode, research-to-deck, and an org template library
*Verevon today: Partial · Effort: L · Plane: Model Plane (slides tool, capability-core templates) with Frontend Plane (slide strip, present mode) and Data Plane (reference assets) · Wave: W3 · Confidence: High*

**What and why.** Put a versioned deck manifest with per-slide state under the existing deck path, validate each slide before delivery, draw a slide strip and present mode from the manifest, and let orgs ship brand templates as skills. Decks are a routine deliverable and nothing checks slide quality today.

**Reference practice.** Decks are produced by 4 of 12 references; research-to-deck chaining and template skills are each unique to one [DOC][SRC]. Manus Slides owns a manifest {deck_id, template_id, slides[{id, state, layout, content, notes}]} that is never hand-edited, with per-slide validation hooks and a present gate [DOC][SRC]. Codex ships templates as skill folders with a manifest, reference asset and preview, picked through a thumbnail form [SRC].

**Verevon today and approach.** Decks exist two ways: python-pptx in code_interpreter (advertised at `apps/Model Plane/rust/services/model-gateway/src/tool_loop.rs:4320`, baked in at `execution-core/Dockerfile:82`) and a browser export of one answer (`apps/Frontend Plane/verevonv3/src/features/chat/lib/chat-export/pptx.ts:1-80`). Missing: any manifest, a render step (no renderer in the image, workspace deleted per call), slide strip, present mode and a template skill kind. Deliver plan H4/H8 first, then add the manifest as a small validated JSON schema rendered by a python-pptx helper, with the SPA drawing strip and present mode from the JSON. Keep validation structural (shape bounds, overflow heuristics, notes present) unless [F-t3-014](#F-t3-014) lands a renderer. Ship templates as org-authored skills with brand tokens through the DeckTheme seam (OD18, OD19); reference files need Data Plane storage.

**Dependencies and risks.** [F-t3-014](#F-t3-014), [F-t3-027](#F-t3-027); a sandbox renderer pushes this to XL; no durable manifest or template library for ZDR threads.

<a id="F-t3-039"></a>
### F-t3-039 · Runtime release safety: signed candidate and separate rollback artifact, rehearsed rollback, deployed audit fix and a release gate
*Verevon today: Partial · Effort: M · Plane: Model Plane (release tooling), Infra Plane for deploy · Wave: W1 · Confidence: Medium*

**What and why.** Run and prove the release tooling that already exists: produce a signed candidate and an independently signed rollback artifact, rehearse rollback against a database snapshot, set numeric canary thresholds, and deploy the audit-publisher fix that sits undeployed. Without a proven rollback Verevon cannot defend its audit and ZDR promises. Quick win.

**Reference practice.** 4 of 12 references ship release safety [SRC]. T3 Code installs the new version beside the old, preflights it, starts a trial child against a database snapshot and rolls back on failure [SRC]. The Claude desktop app ships a signed manifest {version, artifacts[sha256, size], compat} with staged rollout; Hermes restarts drain-first with receipts for interrupted steps [SRC].

**Verevon today and approach.** `apps/Model Plane/scripts/release-artifact.sh:108-180` already has build, verify, restore and deploy subcommands, signed root-manifest verification and rollback-locator rules, and `scripts/coresystem-conformance.sh:100-125` blocks on missing candidate or rollback evidence. Still open: no signed artifact has been produced (`apps/Model Plane/docs/MODEL_PLANE_RELEASE_EVIDENCE.md:1047` reads absent | absent), the thresholds TSV has no canary values, only migrations 0044 and 0045 have revert scripts, and per postmortem 0002 the publisher fix (commit b9ed9197) and requeue function (migration 0043) are built but not deployed, with 4,992 rows unreplayed. Replace the markdown-row grep in the gate with a check of the signed manifest and rollback evidence files; add error-rate, p95 latency and ZDR-residual thresholds with owner approval; script a rehearsal that restores the rollback artifact and applies reverts to a snapshot, with an expand/contract policy for migrations without reverts; deploy the fix and run the requeue.

**Dependencies and risks.** Signing keys and revert scripts; the gate needs an emergency path, and the evidence register is dated (2026-08-17), so state must be re-verified live.

<a id="F-t3-040"></a>
### F-t3-040 · Eval and replay release gates for prompts, tools and routing: recorded-session replay in CI, required eval stages and an eval gate on RoutingPolicy edits
*Verevon today: Partial · Effort: L · Plane: Model Plane (inference-core, eval-lab, orchestrator-core) · Wave: W3 · Confidence: High*

**What and why.** Replay recorded completions through model-gateway in continuous integration (CI), make eval-lab a required stage when prompts or tool schemas change, and put the routing policy behind a candidate, evaluate, promote flow. Verevon edits routing at runtime and changes prompts often; neither path is gated.

**Reference practice.** Replay tests appear in 2 of 12 references and behavioural evals in 3 of 12, a differentiator [SRC]. The DeepSeek harness commits 600 session logs across 227 scenarios, replays them through shipped profiles with an llm-replay provider, tokenises volatile ids, and requires every user-visible change to update a scenario in the same pull request [SRC]. Codex's migration playbook inventories every model usage site and validates against a matrix [SRC].

**Verevon today and approach.** eval-lab-py ships 14 YAML cases and a report writer, but `.github/workflows/eval-lab.yml:1-60` runs only the credential-free selftest and only on eval-lab changes; no comparison or promotion code exists despite the README claim. RoutingPolicy is one platform-global JSONB row set by `apps/Model Plane/rust/services/session-core/src/routing_policy_grpc.rs:90-165` and hot-swapped by `inference-core/src/http_health.rs:126-175` after serde validation only. Start with synthetic completions served by a replay fake at the InferenceCore gRPC boundary (the MockInference pattern at `execution-core/src/runtime_loop/agent.rs:5546`), outside the production binary; do not make [F-t3-034](#F-t3-034) capture a prerequisite. Widen eval-lab path filters to model-gateway prompt and tool-schema files. Gate the global policy as an async candidate-evaluate-promote flow modelled on the SkillPromotion workflow, not a rejected PUT, because live evals need credentials and spend.

**Dependencies and risks.** [F-t3-042](#F-t3-042), [F-t3-067](#F-t3-067); recorded sessions must be synthetic or scrubbed and never ZDR turns; split experiment ids and user-path budgets into separate S or M items.

<a id="F-t3-041"></a>
### F-t3-041 · Gemini provider family (Vertex, EU region) with tool-schema sanitising and a Gemini prompt profile
*Verevon today: Absent · Effort: L · Plane: Model Plane · Wave: W3 · Confidence: High*

**What and why.** Add a Gemini provider to inference-core on Vertex AI in a European Union (EU) region, with thinking-level mapping from the effort ladder, thought-signature replay and a tool-schema sanitiser, exposed in the picker and router for long-context and perception roles. Gemini is named in the product goal and those roles cannot reach it today.

**Reference practice.** 5 of 12 references ship a multi-vendor lineup and 4 of 12 per-family prompt or tool variants [DOC][SRC]. Perplexity web offers one picker across vendors with a Best auto-router and a per-model Thinking toggle [DOC]. Hermes sanitises tool schemas for Gemini and Kimi; Cursor and Codex keep per-family prompt variants; Antigravity maps Gemini thinking levels [SRC].

**Verevon today and approach.** ModelFamily has only OpenAiCompatible and Anthropic (`apps/Model Plane/rust/services/inference-core/src/provider/mod.rs:462-478`); the seeded Gemini row was soft-deleted by `capability-core/migrations/0013_privacy_tier_columns.up.sql` because no adapter serves it, and `model-gateway/src/argument_repair.rs:1-30` only validates call arguments. Prompt branching is a claude-prefix check (`compaction.rs:136-150`), so the prompt-profile half has no baseline and becomes a separate measured item. Do not "re-enable" the catalogue row: it is decorative metadata, and the picker comes from ListModels over the provider chain. Register a new provider with an honest ProviderCapabilities record (Residency::Eu, no ZDR attestation) so the `fallback.rs` deny-by-default gates skip it for ZDR and privacy-floored turns; add pricing and CostBasis or Gemini turns go unmetered. A first slice through the OpenAI-compatible adapter against Vertex is unverified [INF] and needs a short-lived token provider; the native adapter is needed for thought signatures. No Gemini embeddings.

**Dependencies and risks.** [F-t3-029](#F-t3-029), [F-t3-042](#F-t3-042), Vertex residency and ZDR attestation.

<a id="F-t3-042"></a>
### F-t3-042 · Model record v2 and role table: one inference-core model record (context, modalities, effort ladder, lifecycle, price, privacy, residency) rendered by a searchable picker, and purpose-tagged role routing for every inference call
*Verevon today: Partial · Effort: L · Plane: Model Plane (inference-core, model-gateway) with Frontend Plane (picker) · Wave: W3 · Confidence: High*

**What and why.** Extend the thin model record with context window, max output and effort ladder, join price from cost-core, add picker search, and give RoutingPolicy a role dimension that replaces hard-coded side-task models, with a purpose tag on every call. The SPA cannot gate controls per model and side tasks pin constants.

**Reference practice.** 4 of 12 references ship a rich model record, 6 of 12 role slots, 4 of 12 multimodal routing; purpose tags are unique to the DeepSeek harness [SRC]. Codex embeds a remotely refreshed models.json with about 40 fields per model and resolves auxiliary.<task>.{provider, model, effort} for about 20 side tasks through the main resolver [SRC]. T3 Code shows the searchable picker; Hermes the role table [SRC].

**Verevon today and approach.** ListModels returns only id, provider, modality, streaming, features, privacy tier and residency (`apps/Model Plane/proto/model_plane/v1/inference.proto:481-525`), while limits live in ProviderCapabilities (`inference-core/src/provider/mod.rs:395-460`) and prices in cost-core. Title, follow-ups and judge calls are gpt-4o-mini constants (`model-gateway/src/sse.rs:7835-7845,7867,7888-7900`); RoutingPolicy has no role field. Two premises were wrong: the browser planner already resolves through the verevon-balance tier (`execution-core/src/llm_planner.rs:105-135`), and gating by features and provider exists. Split: (1) S-M extend ModelInfo from ProviderCapabilities, join price by id, add search; (2) M role table as a serde-default RoutingPolicy field with a resolver that honours the documented non-reasoning small-budget constraint, filters by the caller's privacy posture and rejects ids absent from the catalogue; (3) S-M purpose tag on InferRequest for metrics and cost attribution.

**Dependencies and risks.** [F-t3-023](#F-t3-023); inference-core owns capability, cost-core pricing, session-core policy: join by id, never copy across planes.

<a id="F-t3-043"></a>
### F-t3-043 · Sandbox egress tiers and a credential-injecting egress proxy, with per-call enforcement reports and shell-grammar approvals
*Verevon today: Partial · Effort: XL · Plane: Model Plane (execution-core, capability-core) with Control Plane (secret store) · Wave: W3 · Confidence: High*

**What and why.** Add a closed egress tier enum (off, registries, allowlisted domains, all) in the Control-signed Space capability decision, enforce it with a real network boundary, show the per-call sandboxed and enforcement result in tool cards, and later inject vaulted credentials so sandboxes see only sentinels. Sandboxes have no network today, which blocks package installs and API work.

**Reference practice.** Egress tiers appear in 3 of 12 references and credential isolation in 5 of 12 [SRC][DOC]. claude.ai offers four tiers with plan-dependent defaults [DOC]; Manus applies admin policy sets with glob and CIDR patterns [SRC]. The DeepSeek harness reports enforcement as full or partial per call, Claude masks credentials with sentinels and per-host injection, and Antigravity parses shell grammar to suggest "allow always" rules [SRC].

**Verevon today and approach.** MpNetworkPolicy has Disabled, AllowAll and AllowDomains (`apps/Model Plane/rust/services/execution-core/src/policy.rs:1-60`), but bwrap unshares the network namespace only for Disabled (`sandbox.rs:170-230`), so a proxy env var is advisory. Control grants space:egress for any non-default claim with no org entitlement (`apps/Control Plane/user-core/internal/spaces/space_capability_decision.go:100-175`), contrary to its design doc. No proxy, credential store or enforcement field exists. Split: (a) S: add sandboxed and enforcement fields to CodeResult from ExecOutcome, shown as a badge; (b) L: tier enum plus allowlist digest in the claim, an explicit Control entitlement, netns or firewall enforcement (or the [F-t3-009](#F-t3-009) VM) reusing Quarry's dns_guard; (c) XL, demote: credential-injecting proxy; (d) M: shell-grammar approvals; versioned writes belong to [F-t3-030](#F-t3-030).

**Dependencies and risks.** [F-t3-009](#F-t3-009), [F-t3-032](#F-t3-032); unknown tier means Disabled; any tier above off is refused for ZDR and high-risk runs.

<a id="F-t3-044"></a>
### F-t3-044 · Inline interactive outputs: sandboxed generative-UI widgets with a host-validated prompt bridge, selection-to-agent actions and attributed image results
*Verevon today: Partial · Effort: M · Plane: Frontend Plane (widget frame, bridge) with Model Plane (widget artifact kind) and Ingestion Plane (image results) · Wave: W4 · Confidence: High*

**What and why.** Render model-written HTML or SVG widgets inline in the transcript using the existing opaque-origin sandbox, with host theme tokens and a narrow postMessage bridge whose follow-ups appear as visible, rate-limited user turns. Selection-to-context and an attributed image strip are separate pieces. Verevon's sandboxed HTML is confined to the Output panel.

**Reference practice.** Inline widgets and point-and-edit each appear in 6 of 12 references [SRC][DOC]. A show_widget tool returns an HTML fragment rendered in a sandboxed iframe under a strict Content Security Policy (CSP), with semantic CSS variables for light and dark, a getState/setState snapshot persisted with the message (up to 16 KiB in Codex), and sendFollowUp gated by a confirmation dialog [SRC][DOC].

**Verevon today and approach.** Model-written HTML already renders through sandboxHtmlDocument (allow-scripts, no allow-same-origin, connect-src none; `apps/Frontend Plane/verevonv3/src/shared/lib/sandbox-html.ts:1-34`) but only in the Output panel (`ChatArtifactPanel.tsx:495-520`); artifact kinds stop at document, code and html (`apps/Model Plane/rust/services/model-gateway/src/sse_events.rs:870-895`). No bridge or selection item exists, and Quarry image results reach only the dashboard search panel. Widget core (M): add a widget kind to the exhaustive ConversationNode registry, mount the iframe only when the artifact is complete and keep it in a keyed cache like ChatChart, because nodes are re-derived on every streamed token. Authenticate bridge events by event.source equal to the iframe contentWindow, since an opaque origin posts "null". Route widget prompts through the normal composer submit path so ZDR, privacy floor and run policy apply. Selection-to-context (S to M) goes with plan I17/OD19; the image strip (S) is a BFF fetch of /api/v1/search/images for web-search turns.

**Dependencies and risks.** [F-t3-031](#F-t3-031), [F-t3-052](#F-t3-052); no widget snapshots or image results for ZDR threads; thumbnails with links only.

<a id="F-t3-045"></a>
### F-t3-045 · MCP Apps, elicitation forms and a hosted artifact runtime on a dedicated user-content origin
*Verevon today: Absent · Effort: L · Plane: Frontend Plane (frame, origin) with Model Plane (MCP host, gate) and Application Plane (storage) and Control Plane (viewer identity) · Wave: W4 · Confidence: Medium*

**What and why.** Render tool-supplied user interfaces (Model Context Protocol (MCP) Apps, ui:// resources) and MCP elicitation forms inside the existing sandboxed frame, with JSON-RPC over postMessage whose tools/call passes the capability gate and approvals. Defer the dedicated user-content origin, viewer-bound connectors and viewer key-value store.

**Reference practice.** MCP Apps appear in 6 of 12 references; elicitation and an artifact host bridge in 2 of 12 each [SRC][DOC]. Tool metadata _meta.ui.resourceUri names a ui:// resource; the host fetches the text/html;profile=mcp-app resource and renders it in an iframe on a separate origin with sandbox=allow-scripts and a CSP from _meta [SRC][DOC]. Codex ships ext-apps and form elicitation [SRC]; ChatGPT web documents the JSON-RPC and CSP model [DOC].

**Verevon today and approach.** A grep for ui://, resourceUri, ext-apps, elicitation/create and sampling/createMessage across Model Plane, frontend and gateway found nothing; the MCP client declares empty capabilities and opens a fresh session per call (`apps/Model Plane/rust/services/model-gateway/src/mcp_jsonrpc.rs:30-42`, `mcp_http.rs:1-60`). The "elicitation" at `tool_loop.rs:648-656` is prompt guidance for missing arguments. The only substrate is ArtifactHtmlPreview (`apps/Frontend Plane/verevonv3/src/features/chat/components/ChatArtifactPanel.tsx:496-530`). The opaque-origin srcdoc frame can host a bridge authenticated by event.source; a separate origin is needed only for same-origin web APIs, SRI-loaded libraries or shareable URLs, an ADR decision [INF, medium confidence]. Merge the bridge with [F-t3-044](#F-t3-044), split elicitation (client capabilities plus the existing approval pause, handled inside the tools/call SSE stream) from MCP Apps, and build an org-restricted per-app CSP. MCP tools are not callable today: the inline loop refuses mcp_catalog and mcp_call (tool_loop.rs:3686-3697) and trusted_capability_id returns None for mcp__ names; [F-t3-003](#F-t3-003) makes them callable through the signed gate with side-effect classes.

**Dependencies and risks.** [F-t3-003](#F-t3-003), [F-t3-044](#F-t3-044), an ADR for the origin; every app-initiated call is attributed to the viewer, never the author; no persistence or key-value store under ZDR.

<a id="F-t3-046"></a>
### F-t3-046 · Sharing, replay, export and import: governed snapshot links, org-only default, and conversation export and import
*Verevon today: Partial · Effort: XL · Plane: Model Plane (snapshots, export, import), Control Plane (grants, audit, admin policy) · Wave: W4 · Confidence: High*

**What and why.** Add an immutable, redacted thread snapshot with org-internal visibility, Control-checked grants and audit, a redacted replay view, an owner-only whole-conversation export, and an importer for ChatGPT and Claude export files. Org-only sharing is expected in B2B and import lowers switching cost.

**Reference practice.** Sharing appears in 8 of 12 references and export or import in 6 of 12 [SRC][DOC]. Manus shares an immutable snapshot up to a message (artifacts included, raw tool payloads excluded unless in-org) with a visibility enum, an audit entry, replay with skip-to-result, and admin policy where the strictest setting wins [SRC]. claude.ai and Perplexity fork a follow-up under the viewer [DOC]; Manus and the Claude desktop app run conversion jobs and importers that can remove an import run [SRC].

**Verevon today and approach.** No chat share route exists; user-core grants cover documents only and `apps/Control Plane/user-core/internal/authztaxonomy/resource_taxonomy.json:1-17` lists document as the only ownable type, so a snapshot needs a new classified type kept in parity across the Go embed, Rust mirror and lint script. Replay routes (`apps/Model Plane/rust/services/model-gateway/src/http_routes.rs:1243-1340`) are envelope-only by design, so replay must be a new redacted projection. There is no ForkThread RPC (`artifacts.rs:184-204` forks only artifact counters) and no import origin in THREAD_ORIGINS. Per-answer Word, Excel, PowerPoint and print-to-PDF exports ship in the browser (`ChatMessages.tsx:610-630`). Split into three tracks: share snapshot (L, owner-gated by OD19 and plan I19, view-only until I8/I20 lands a fork), export (M, audited through the session audit outbox), import (M, new origin value plus DB CHECK, turns marked untrusted). Refuse all three for ZDR threads; redact or refuse connector content.

**Dependencies and risks.** [F-t3-012](#F-t3-012), ADR on the share model (OD19); imported content may carry prompt injections.

<a id="F-t3-047"></a>
### F-t3-047 · Event-triggered automations: normalised connector events from Ingestion wake budgeted runs, with wake gates, dry runs, blueprints and consent-first suggestions
*Verevon today: Absent · Effort: XL · Plane: Model Plane (capability-core triggers) with Ingestion Plane (integration-corev2 events) · Wave: W4 · Confidence: Medium*

**What and why.** Let connector events published by Ingestion wake budgeted Do runs through a trigger record bound to one connection, Space and member, with wake gates, dry runs and per-fire reauthorization; defer blueprints and suggestions. This turns agents from on-demand into proactive, the natural step after scheduled runs.

**Reference practice.** Event triggers appear in 5 of 12 references, wake gates in 3 of 12 [SRC][DOC]. Cursor runs an automations service with event triggers, LLM filtering and dry runs; Hermes adds a source-hash wake gate, a silent sentinel, per-job notes and capped, de-duplicated suggestions [SRC]. ChatGPT MCP Events subscribe with a callback URL and signing secret, verify signed webhooks with a 256 KiB cap, and start a run in the bound conversation [DOC].

**Verevon today and approach.** integration-corev2 publishes webhook_received carrying only eventType, webhookEventId and normalizedBy (`apps/Ingestion Plane/integration-corev2/internal/api/server.go:1050-1100`); the only consumer is Application Plane conversation-core. capability-core has cron_schedules with a per-fire FireAuthorizer (`apps/Model Plane/go/services/capability-core/internal/cron/sweeper.go:14-47`) and a Watch primitive with one process_output adapter and a one-resource, one-Space, one-member model (`internal/watch/watch.go:1-140`), but no trigger records, filters, wake gates or dry runs. Model a trigger as a new Watch source kind or a cron_schedules-like row keyed by (connection_id, Space, member) that implements FireAuthorizer. Consume webhook_received through a durable JetStream consumer (new nats-provisioner binding), fetch the payload by webhookEventId through the integration-corev2 API, dedupe on it as the fire key, classify the payload untrusted before any LLM predicate, and start the run through the scheduled-run path under run budget and org quota caps. Only three event types exist in code and no mail-received event [INF, pattern search], so mail triggers need an Ingestion change first.

**Dependencies and risks.** [F-t3-018](#F-t3-018), [F-t3-002](#F-t3-002); event storms need rate limits; ZDR must come from policy because there is no user turn; trimmed to trigger, consumer, wake gate and budgeted run it is L.
