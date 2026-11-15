## 9. Roadmap and decisions

This section turns the 98 recommendations into an order of work. Tiers, efforts and waves are the verified values: 22 priority, 46 recommended and 30 nice-to-have items after verification, with 40 effort estimates revised and one item demoted from priority to recommended ([F-t3-016](#F-t3-016)). Where an effort changed, the table shows the verified value and the original in brackets. Dependencies come from each item's dependency list; "ADR" means an Architecture Decision Record that must exist before the item starts, and "OD" refers to the open decisions numbered in the chat gap-closure plan (apps/Frontend Plane/verevonv3/apps/verevon-web/plans/system-audits/CHAT_GAP_CLOSURE_PLAN_2026-10-01.md). Sequencing is the audit's judgement [INF].

### 9.1 Implementation waves

#### Wave W1: make what exists trustworthy

Goal: make the existing runtime correct and provable before adding surface area: one run context for Do runs, durable and replica-safe runs, declared tool risk so MCP (Model Context Protocol) tools become callable, audit delivery, Zero Data Retention (ZDR) live, learning and memory governance, cache and cost basics, render performance, and release and CI guardrails.

| ID | Title | Tier | Effort | Plane | Dependencies |
|---|---|---|---|---|---|
| [F-t3-001](#F-t3-001) | One server-assembled run context for Do, Plan and scheduled runs | priority | M | Model Plane | [F-t3-022](#F-t3-022), [F-t3-020](#F-t3-020), ADR-0003 precedence (OD19), execution.proto steward |
| [F-t3-002](#F-t3-002) | Durable agent loop | priority | L | Model Plane | [F-t3-016](#F-t3-016), OD8 renewable delegated bearer, Control reauthorizer |
| [F-t3-003](#F-t3-003) | Declared tool side-effect classes and per-tool policy; MCP callable | priority | L | Model Plane | continuation descriptor key provisioned, integration-corev2 catalogue metadata |
| [F-t3-004](#F-t3-004) | Deliver the AI-action audit trail, then tamper evidence and a compliance API | priority | L (was M) | Model Plane, Control Plane | nats-provisioner stream manifest, [F-t3-039](#F-t3-039) |
| [F-t3-005](#F-t3-005) | Unblock Zero Data Retention | priority | M (was S) | Model Plane, Control Plane | provider contract evidence (Z-40) |
| [F-t3-007](#F-t3-007) | Govern and prove the skill-learning loop | priority | L | Model Plane, Control Plane, Frontend Plane | [F-t3-020](#F-t3-020), owner decision on the three-tier library, Control decision signing |
| [F-t3-016](#F-t3-016) | Multi-replica-safe run control | recommended (was priority) | M (was L) | Model Plane, Frontend Plane | Redis in every deploy, NATS mp.v1 subjects |
| [F-t3-017](#F-t3-017) | Attachments v2 | priority | L | Model Plane, Data Plane, Frontend Plane | Data Plane upload route, OD18 caps, plan H9 OCR |
| [F-t3-020](#F-t3-020) | User and org memory controls | priority | L (was M) | Model Plane, Control Plane, Frontend Plane | Control user settings and org policy |
| [F-t3-021](#F-t3-021) | Transcript performance | priority | M | Frontend Plane, Model Plane | plan I5 |
| [F-t3-022](#F-t3-022) | Prompt-cache discipline | priority | M | Model Plane | inference-core usage reporting, compaction lineage id |
| [F-t3-023](#F-t3-023) | Org and role model entitlements | priority | L (was M) | Control Plane, Model Plane, Frontend Plane | ADR model entitlement ownership, Control policy schema |
| [F-t3-029](#F-t3-029) | Canonical reasoning-effort ladder per provider wire format | recommended | M (was S) | Model Plane | none |
| [F-t3-039](#F-t3-039) | Runtime release safety | recommended | M | Model Plane | signing keys, migration revert scripts |
| [F-t3-066](#F-t3-066) | UI quality gates | recommended | S (was M) | Frontend Plane | none |
| [F-t3-067](#F-t3-067) | Contract and architecture guardrails in CI | recommended | L | Frontend Plane | none |

#### Wave W2: close the agent-loop and governance gaps

Goal: fix what users feel in chat and Do mode: approvals v2, plan-continue and ask_user, working connectors and agent browser, the history foundation, Spaces as projects, verified Office deliverables, scheduled Do runs, the attention model, file tools, the action registry and the org policy registry, and the first phase of the sandbox-computer seam.

| ID | Title | Tier | Effort | Plane | Dependencies |
|---|---|---|---|---|---|
| [F-t3-006](#F-t3-006) | Approvals v2 | priority | L | Model Plane, Frontend Plane | [F-t3-003](#F-t3-003), [F-t3-002](#F-t3-002), [F-t3-066](#F-t3-066) |
| [F-t3-008](#F-t3-008) | Agent browser on real broker grants with per-site consent | priority | XL (was L) | Model Plane, Ingestion Plane, Control Plane | [F-t3-006](#F-t3-006), OD13, plan E1/E2/E4 |
| [F-t3-009](#F-t3-009) | Per-thread sandbox computer behind a SandboxBackend seam | priority | XL | Model Plane, Control Plane | [F-t3-002](#F-t3-002), ADR sandbox backend, VM metering |
| [F-t3-010](#F-t3-010) | Connected apps that work in chat | priority | L | Model Plane, Ingestion Plane, Control Plane | [F-t3-003](#F-t3-003), [F-t3-006](#F-t3-006), OD10/OD12, OAuth app verification |
| [F-t3-011](#F-t3-011) | Finish human-in-the-loop | priority | L | Model Plane, Frontend Plane | [F-t3-002](#F-t3-002), [F-t3-001](#F-t3-001), [F-t3-016](#F-t3-016), [F-t3-015](#F-t3-015) |
| [F-t3-012](#F-t3-012) | Conversation history foundation in session-core | priority | L | Model Plane, Frontend Plane | [F-t3-016](#F-t3-016), OD19 branch semantics |
| [F-t3-013](#F-t3-013) | Make Spaces the chat project | priority | L | Application Plane, Model Plane, Data Plane | [F-t3-001](#F-t3-001), [F-t3-012](#F-t3-012), ADR Spaces as project (OD19) |
| [F-t3-014](#F-t3-014) | Verified Office deliverables through SKILL.md bundles | priority | XL (was L) | Model Plane, Frontend Plane | [F-t3-017](#F-t3-017), OD18 |
| [F-t3-015](#F-t3-015) | Attention model for long-running agents | priority | XL | Model Plane, Application Plane, Frontend Plane | [F-t3-016](#F-t3-016), [F-t3-012](#F-t3-012) |
| [F-t3-018](#F-t3-018) | Scheduled runs as governed, tool-using Do runs | priority | L | Model Plane, Control Plane, Application Plane | [F-t3-001](#F-t3-001), [F-t3-002](#F-t3-002), [F-t3-003](#F-t3-003), ADR unattended authority |
| [F-t3-025](#F-t3-025) | Open the typed action registry to the agent | recommended | M | Frontend Plane, Model Plane | [F-t3-003](#F-t3-003), [F-t3-006](#F-t3-006), AIF M1 trace |
| [F-t3-028](#F-t3-028) | Live run-cost meter and pre-run estimate | recommended | L | Model Plane, Control Plane, Frontend Plane | [F-t3-022](#F-t3-022), [F-t3-042](#F-t3-042) |
| [F-t3-030](#F-t3-030) | Workspace file tools for Do mode | recommended | M | Model Plane | [F-t3-003](#F-t3-003), OD2 Do mode on the product route |
| [F-t3-032](#F-t3-032) | Org agent policy registry | recommended | XL (was L) | Control Plane, Model Plane, Frontend Plane | [F-t3-003](#F-t3-003), [F-t3-006](#F-t3-006) |
| [F-t3-033](#F-t3-033) | Content-safety hardening | recommended | L | Model Plane | [F-t3-020](#F-t3-020), false-positive evaluation set |
| [F-t3-049](#F-t3-049) | Typed run lifecycle events and status cues | recommended | L (was M) | Model Plane, Frontend Plane | SSE taxonomy gate |
| [F-t3-063](#F-t3-063) | Loop guard, durable org hooks, universal tool-output handles | recommended | L (was M) | Model Plane | [F-t3-032](#F-t3-032) |

#### Wave W3: research, creation and personalisation

Goal: deliver the research, creation and personalisation table-stakes on the stabilised loop: deep and wide research, citations to passages, readable long runs, the artifact canvas, decks, the Agents and Automations hubs, takeover, goals, the skills lifecycle, scoped memory, the model record and Gemini, eval gates, egress tiers and the microVM sandbox.

| ID | Title | Tier | Effort | Plane | Dependencies |
|---|---|---|---|---|---|
| [F-t3-019](#F-t3-019) | Real Agents and Automations hubs | priority | XL | Model Plane, Application Plane, Frontend Plane | [F-t3-018](#F-t3-018), [F-t3-025](#F-t3-025), [F-t3-060](#F-t3-060) |
| [F-t3-026](#F-t3-026) | Human takeover and hand-back from live views | recommended | M (was L) | Model Plane, Ingestion Plane, Frontend Plane | [F-t3-008](#F-t3-008), [F-t3-002](#F-t3-002) |
| [F-t3-027](#F-t3-027) | Deep research v2 | recommended | XL (was L) | Model Plane | [F-t3-054](#F-t3-054), [F-t3-010](#F-t3-010) |
| [F-t3-031](#F-t3-031) | Artifact canvas | recommended | XL | Model Plane, Frontend Plane, Application Plane, Data Plane | [F-t3-012](#F-t3-012), [F-t3-052](#F-t3-052) |
| [F-t3-034](#F-t3-034) | Trajectory ledger with request capture and timing waterfall | recommended | XL | Model Plane, Frontend Plane | [F-t3-049](#F-t3-049), ZDR resolver |
| [F-t3-036](#F-t3-036) | Wide research | recommended | L | Model Plane | [F-t3-028](#F-t3-028), [F-t3-002](#F-t3-002), OD7 |
| [F-t3-037](#F-t3-037) | Persistent goals with a token budget | recommended | XL (was L) | Model Plane | [F-t3-002](#F-t3-002), [F-t3-001](#F-t3-001), [F-t3-018](#F-t3-018), OD8 |
| [F-t3-038](#F-t3-038) | Deck pipeline with brand templates | recommended | L | Model Plane, Frontend Plane, Data Plane | [F-t3-014](#F-t3-014), [F-t3-027](#F-t3-027) |
| [F-t3-040](#F-t3-040) | Eval and replay release gates | recommended | L | Model Plane | [F-t3-042](#F-t3-042), [F-t3-067](#F-t3-067) |
| [F-t3-041](#F-t3-041) | Gemini provider family (Vertex, EU region) | recommended | L | Model Plane | [F-t3-029](#F-t3-029), [F-t3-042](#F-t3-042), Vertex ZDR and residency attestation |
| [F-t3-042](#F-t3-042) | Model record v2 and role table | recommended | L | Model Plane, Frontend Plane | [F-t3-023](#F-t3-023) |
| [F-t3-043](#F-t3-043) | Sandbox egress tiers and credential-injecting egress proxy | recommended | XL (was L) | Model Plane, Control Plane | [F-t3-009](#F-t3-009), [F-t3-032](#F-t3-032) |
| [F-t3-048](#F-t3-048) | Readable long runs | recommended | M | Frontend Plane, Model Plane | [F-t3-021](#F-t3-021), [F-t3-066](#F-t3-066) |
| [F-t3-052](#F-t3-052) | Composer: typed tokens, drafts, thread references | recommended | L | Frontend Plane, Model Plane | [F-t3-012](#F-t3-012), [F-t3-017](#F-t3-017), OD19 |
| [F-t3-053](#F-t3-053) | Claim-level citations and research deliverables | recommended | M | Model Plane, Frontend Plane, Data Plane | [F-t3-027](#F-t3-027) |
| [F-t3-054](#F-t3-054) | Web reading upgrade | recommended | M (was L) | Model Plane | none |
| [F-t3-055](#F-t3-055) | Web access governance and provider transparency | recommended | L (was M) | Control Plane, Model Plane | [F-t3-032](#F-t3-032) |
| [F-t3-056](#F-t3-056) | Connectors: suggestion cards and editable write confirmation | recommended | L | Model Plane | [F-t3-010](#F-t3-010), [F-t3-003](#F-t3-003) |
| [F-t3-058](#F-t3-058) | One versioned lifecycle for authored behaviours | recommended | XL (was L) | Model Plane, Frontend Plane | [F-t3-014](#F-t3-014), [F-t3-007](#F-t3-007) |
| [F-t3-060](#F-t3-060) | Scoped memory for Spaces, Projects and named agents | recommended | L | Model Plane, Frontend Plane | [F-t3-013](#F-t3-013), [F-t3-001](#F-t3-001), [F-t3-020](#F-t3-020) |
| [F-t3-061](#F-t3-061) | Personalisation that tailors itself | recommended | L (was M) | Model Plane, Control Plane, Frontend Plane | [F-t3-001](#F-t3-001), [F-t3-020](#F-t3-020), [F-t3-022](#F-t3-022) |
| [F-t3-062](#F-t3-062) | Enterprise compliance posture | recommended | XL (was L) | Control Plane, Model Plane, Support Plane | [F-t3-005](#F-t3-005), [F-t3-004](#F-t3-004) |
| [F-t3-068](#F-t3-068) | Policy and data-flow transparency | recommended | L (was M) | Frontend Plane, Control Plane | [F-t3-006](#F-t3-006), [F-t3-005](#F-t3-005) |

#### Wave W4: a computer-using platform

Goal: computer use inside the sandbox desktop, the live computer panel, inline widgets and MCP Apps, sharing, event triggers, replay, images as a governed tool, custom connectors, plugins, the public Agent API and bring-your-own providers.

| ID | Title | Tier | Effort | Plane | Dependencies |
|---|---|---|---|---|---|
| [F-t3-024](#F-t3-024) | Computer use inside the agent's own sandbox desktop | recommended | XL | Model Plane, Endpoint Plane | [F-t3-009](#F-t3-009), [F-t3-035](#F-t3-035), [F-t3-026](#F-t3-026), ADR computer-use scope |
| [F-t3-035](#F-t3-035) | Live computer panel with typed viewers and PTY exec | recommended | L | Frontend Plane, Model Plane | [F-t3-009](#F-t3-009), [F-t3-008](#F-t3-008) |
| [F-t3-044](#F-t3-044) | Inline interactive outputs | recommended | M (was L) | Frontend Plane, Model Plane, Ingestion Plane | [F-t3-031](#F-t3-031), [F-t3-052](#F-t3-052) |
| [F-t3-045](#F-t3-045) | MCP Apps, elicitation forms, hosted artifact runtime | recommended | L | Frontend Plane, Model Plane, Application Plane, Control Plane | [F-t3-003](#F-t3-003), [F-t3-044](#F-t3-044), ADR user-content origin |
| [F-t3-046](#F-t3-046) | Sharing, replay, export and import | recommended | XL (was L) | Model Plane, Control Plane | [F-t3-012](#F-t3-012), ADR share model (OD19) |
| [F-t3-047](#F-t3-047) | Event-triggered automations | recommended | XL (was L) | Model Plane, Ingestion Plane | [F-t3-018](#F-t3-018), [F-t3-002](#F-t3-002) |
| [F-t3-050](#F-t3-050) | Scrubbable browser-run replay | recommended | M | Frontend Plane, Ingestion Plane, Model Plane | [F-t3-008](#F-t3-008), [F-t3-034](#F-t3-034) |
| [F-t3-051](#F-t3-051) | Make images a governed agent tool | recommended | XL (was L) | Model Plane, Frontend Plane, Control Plane | [F-t3-017](#F-t3-017), [F-t3-041](#F-t3-041) |
| [F-t3-057](#F-t3-057) | Custom connectors | recommended | L | Model Plane, Ingestion Plane | [F-t3-003](#F-t3-003), [F-t3-043](#F-t3-043) |
| [F-t3-059](#F-t3-059) | Plugin manifest loader with supply-chain pins | recommended | L | Model Plane, Control Plane | [F-t3-003](#F-t3-003), [F-t3-058](#F-t3-058), [F-t3-032](#F-t3-032) |
| [F-t3-064](#F-t3-064) | Public, versioned Agent API with origin-tagged runs | recommended | L | Model Plane, Control Plane, Channel Plane | [F-t3-002](#F-t3-002), [F-t3-016](#F-t3-016), [F-t3-067](#F-t3-067) |
| [F-t3-065](#F-t3-065) | Declarative provider registry and bring-your-own connections | recommended | XL | Model Plane | [F-t3-042](#F-t3-042), ADR model entitlement ownership |

#### Wave W5: nice-to-have backlog

Goal: schedule these on demand or when a dependency makes them cheap.

| ID | Title | Tier | Effort | Plane | Dependencies |
|---|---|---|---|---|---|
| [F-t3-069](#F-t3-069) | Rewind with undo preview, side chat, spin-off tasks | nice-to-have | M (was L) | Model Plane, Frontend Plane | [F-t3-012](#F-t3-012), [F-t3-031](#F-t3-031) |
| [F-t3-070](#F-t3-070) | Tool efficiency at scale (deferred index, code mode) | nice-to-have | XL (was L) | Model Plane | [F-t3-003](#F-t3-003), [F-t3-009](#F-t3-009) |
| [F-t3-071](#F-t3-071) | Coordinator-mode orchestration on Temporal | nice-to-have | M (was L) | Model Plane, Frontend Plane | [F-t3-002](#F-t3-002), [F-t3-019](#F-t3-019) |
| [F-t3-072](#F-t3-072) | Resilient routing | nice-to-have | M (was L) | Model Plane | [F-t3-042](#F-t3-042), [F-t3-073](#F-t3-073) |
| [F-t3-073](#F-t3-073) | Per-step routing decision and model continuity | nice-to-have | L | Model Plane | [F-t3-042](#F-t3-042) |
| [F-t3-074](#F-t3-074) | Editable permission rules, CEL policy, MCP tool gate | nice-to-have | L | Model Plane | [F-t3-006](#F-t3-006), [F-t3-032](#F-t3-032) |
| [F-t3-075](#F-t3-075) | Privacy by construction and a closed telemetry contract | nice-to-have | L | Model Plane, Control Plane | [F-t3-067](#F-t3-067), [F-t3-004](#F-t3-004) |
| [F-t3-076](#F-t3-076) | Changed-files card and review gate before promotion | nice-to-have | L (was M) | Model Plane, Frontend Plane | [F-t3-030](#F-t3-030), [F-t3-031](#F-t3-031) |
| [F-t3-077](#F-t3-077) | One action-registry-driven command surface | nice-to-have | M | Frontend Plane | [F-t3-012](#F-t3-012), [F-t3-025](#F-t3-025) |
| [F-t3-078](#F-t3-078) | Feedback surfaces and limit notices | nice-to-have | S (was M) | Frontend Plane | [F-t3-066](#F-t3-066) |
| [F-t3-079](#F-t3-079) | Typed feature-flag and settings registry | nice-to-have | M | Control Plane | [F-t3-039](#F-t3-039) |
| [F-t3-080](#F-t3-080) | Library of generated and uploaded files | nice-to-have | M | Data Plane, Control Plane | [F-t3-031](#F-t3-031) |
| [F-t3-081](#F-t3-081) | Teach mode | nice-to-have | XL (was L) | Ingestion Plane, Model Plane | [F-t3-008](#F-t3-008), [F-t3-026](#F-t3-026), [F-t3-007](#F-t3-007) |
| [F-t3-082](#F-t3-082) | Upgrade voice input | nice-to-have | L (was M) | Model Plane, Frontend Plane | none |
| [F-t3-083](#F-t3-083) | Run admission controls | nice-to-have | L (was M) | Control Plane, Model Plane | [F-t3-002](#F-t3-002), [F-t3-028](#F-t3-028) |
| [F-t3-084](#F-t3-084) | Per-Space environment specs and prebuilt images | nice-to-have | L | Model Plane | [F-t3-009](#F-t3-009), [F-t3-043](#F-t3-043) |
| [F-t3-085](#F-t3-085) | Sandbox computer extensions | nice-to-have | L | Model Plane | [F-t3-009](#F-t3-009), [F-t3-008](#F-t3-008) |
| [F-t3-086](#F-t3-086) | Review as a run | nice-to-have | M | Model Plane, Frontend Plane | [F-t3-030](#F-t3-030) |
| [F-t3-087](#F-t3-087) | Multi-file app build and preview | nice-to-have | L | Model Plane | [F-t3-030](#F-t3-030), [F-t3-009](#F-t3-009), [F-t3-045](#F-t3-045) |
| [F-t3-088](#F-t3-088) | Code edit quality on demand | nice-to-have | S (was M) | Model Plane | [F-t3-030](#F-t3-030) |
| [F-t3-089](#F-t3-089) | Use Verevon from other agents and work tools | nice-to-have | XL (was L) | Channel Plane | [F-t3-025](#F-t3-025), [F-t3-064](#F-t3-064), Channel Plane ADR |
| [F-t3-090](#F-t3-090) | Time-based media on demand | nice-to-have | XL (was L) | Data Plane, Frontend Plane | [F-t3-017](#F-t3-017), [F-t3-041](#F-t3-041) |
| [F-t3-091](#F-t3-091) | Data-connected dashboards and label-aware document policy | nice-to-have | L | Ingestion Plane | [F-t3-017](#F-t3-017) |
| [F-t3-092](#F-t3-092) | Model Council | nice-to-have | M | Model Plane | [F-t3-028](#F-t3-028) |
| [F-t3-093](#F-t3-093) | Grow the corpus from research | nice-to-have | L (was M) | Data Plane | [F-t3-054](#F-t3-054) |
| [F-t3-094](#F-t3-094) | Self-maintained team knowledge wiki | nice-to-have | XL | Data Plane | [F-t3-007](#F-t3-007) |
| [F-t3-095](#F-t3-095) | Second-order latency and cache levers | nice-to-have | L | Model Plane | [F-t3-022](#F-t3-022), [F-t3-042](#F-t3-042) |
| [F-t3-096](#F-t3-096) | Operability long tail | nice-to-have | M | Model Plane, Control Plane | [F-t3-004](#F-t3-004), [F-t3-075](#F-t3-075) |
| [F-t3-097](#F-t3-097) | Tighten-only runtime monitors | nice-to-have | XL | Control Plane | [F-t3-003](#F-t3-003), [F-t3-033](#F-t3-033), ADR LLM on the permission path |
| [F-t3-098](#F-t3-098) | Reader and appearance polish | nice-to-have | M (was S) | Frontend Plane, Control Plane | [F-t3-021](#F-t3-021), [F-t3-066](#F-t3-066) |

### 9.2 Quick wins

Twelve items carry the quick-win flag. Nine of them stay at S or M after verification; the table lists them with the reason they are cheap. Several are "finish what is built but dark": the code exists and is not deployed, not enabled or not measured.

| ID | Title | Effort | Wave | Why it is cheap |
|---|---|---|---|---|
| [F-t3-001](#F-t3-001) | One server-assembled run context | M | W1 | The BFF already injects org, Space and persona text onto the agentic request and model-gateway composes it for Ask turns; the agentic branch simply returns before composition (apps/Model Plane/rust/services/model-gateway/src/sse.rs:1405-1500), so the fix is one composed field on RunAgentRequest plus bounded history. |
| [F-t3-005](#F-t3-005) | Unblock Zero Data Retention | M | W1 | The design is built; the attestation itself is setting environment fields and a digest. The M comes from collecting contract evidence, deploying the uncommitted waves in order and running two live passes. |
| [F-t3-021](#F-t3-021) | Transcript performance | M | W1 | Part of the benchmark exists at unit level and plan I5 (block-level streaming render) is already specified; virtualisation and paged reads are conditional follow-ups. |
| [F-t3-022](#F-t3-022) | Prompt-cache discipline | M | W1 | Cache telemetry is already stored, so the metrics step shrinks to S; the OpenAI stable-prefix order is built and only needs enabling and an eval rerun. |
| [F-t3-029](#F-t3-029) | Canonical reasoning-effort ladder | M (was S) | W1 | A well-understood code gap: the OpenAI-family path ignores the user's effort choice. S for that slice; M only because the full ladder needs a proto field and three adapters. |
| [F-t3-039](#F-t3-039) | Runtime release safety | M | W1 | Signing, rollback-artifact validation and a conformance gate are built; what remains is thresholds, a rehearsal script, a migration policy and operator deployment with keys. |
| [F-t3-025](#F-t3-025) | Open the typed action registry to the agent | M | W2 | Risk metadata and per-run scoped authority exist in part; the framework tranche is classification fields, a CI ratchet and generalising the existing Control decision. Each further action admission is separate owner-plane work. |
| [F-t3-030](#F-t3-030) | Workspace file tools for Do mode | M | W2 | The hydrated Space workspace and CAS (content-addressed storage) merge are real; read, grep and glob are S and only the guarded edit engine is M. |
| [F-t3-050](#F-t3-050) | Scrubbable browser-run replay | M | W4 | Step timeline, rationale and screenshots already exist as events; the scrubber alone is S and the clip is assembled from existing screenshots. |

Three flagged items rose to L during verification and are no longer whole-item quick wins, but each has a cheap first slice: [F-t3-004](#F-t3-004) Phase 0 (deploy the built audit outbox fix) is S, while the hash chain, compliance API and SIEM (security information and event management) export are M to L; [F-t3-020](#F-t3-020) is M for a user-level on/paused/off switch plus reset, with org policy and learning-review gating making it L; [F-t3-023](#F-t3-023) is M for an org allow/deny list on the existing org_quota pull pattern, with role and plan resolution making it L.

### 9.3 Decisions needed before building (ADR candidates)

Ten topics touch standing rules (plane ownership, ZDR propagation, Space authority, "the Model proposes and Quarry executes", no LLM on the permission path). Each needs an ADR or an owner ruling before implementation starts.

1. **Sandbox computer backend and thread-scoped workspace.** Question: does Verevon move from hermetic per-call bwrap processes to long-lived microVMs with a thread-scoped workspace, and under what retention, ACL and billing rules? Items: [F-t3-009](#F-t3-009), [F-t3-024](#F-t3-024), [F-t3-035](#F-t3-035), [F-t3-043](#F-t3-043), [F-t3-084](#F-t3-084), [F-t3-085](#F-t3-085). Options: keep bwrap and add only a thread workspace on it (phase 1, about M); or add a SandboxBackend seam and a microVM backend (self-hosted Firecracker or Kata, or an attested managed provider). The decision ledger of 2026-08-22 rejects a SandboxBackend trait until a second real backend is planned, so the ADR must reverse that first, show ZDR snapshot destruction, prove that a thread workspace never aliases or bypasses Space ACLs, name the owner of guest images and define VM-second metering. Recommendation from the data: phase 1 on bwrap first, ADR before any microVM work.
2. **Computer-use scope.** Question: is desktop control limited to Verevon's sandbox VM, and is user-desktop control permanently out of scope or only parked? Items: [F-t3-024](#F-t3-024), [F-t3-026](#F-t3-026). Options: sandbox only; or sandbox plus Endpoint Plane with consent tiers. The data supports sandbox only, keeping Model Plane as proposer and a lease-bound guest executor, with every action routed through capability_policy (an unmapped tool id fails closed, so a new capability id is needed).
3. **Spaces as the chat project container, instruction precedence and multi-author authority (OD19).** Question: are Spaces the project container for chat, what is the precedence of org, Space, project and persona instructions, and whose delegated token runs a collaborator-triggered turn? Items: [F-t3-013](#F-t3-013), [F-t3-001](#F-t3-001), [F-t3-060](#F-t3-060), [F-t3-061](#F-t3-061), [F-t3-012](#F-t3-012). Options: build projects on Spaces; or add a second container (plan I21 server-synced project folders). Chat history is filtered by thread origin "chat" and Space conversations live in the Space timeline today, so making Spaces the chat project reverses a design that was kept deliberately. Recommendation: Spaces, not a second container, with the caller's Space role re-resolved server-side and never a client-supplied space_ref.
4. **Agent-browser domain policy ownership and model-blind credentials (OD13).** Question: does the broker, Control or Quarry own the domain fence and per-site read/act consent, and how does Quarry substitute vault secrets so the Model never sees them? Items: [F-t3-008](#F-t3-008), [F-t3-026](#F-t3-026), [F-t3-050](#F-t3-050), [F-t3-081](#F-t3-081). The verification data points one way: admin allow and block lists belong to Control, Space decisions may only narrow (block wins), grants are minted under the user's delegated BrowserBroker bearer and never from request JSON, and Quarry stays the final enforcer.
5. **Unattended authority for scheduled, goal and trigger runs (owner ruling 2026-09-28, OD8, D10).** Question: under what stored, expiring consent contract may unattended runs use tools and write, and how are delegated credentials renewed when the bearer lives 300 s? Items: [F-t3-018](#F-t3-018), [F-t3-037](#F-t3-037), [F-t3-047](#F-t3-047), [F-t3-002](#F-t3-002), [F-t3-019](#F-t3-019). Options: a Control-committed template or action schema carrying approved capabilities, an expiry and a ZDR refusal, re-verified at fire and per step; or a service-principal shortcut. The data rejects the shortcut: the signed decision already binds digest, expiry and ZDR refusal, so the contract belongs in Control, not a new Model Plane table.
6. **Share model, public links and artifact hosting origin under ZDR and residency.** Question: which content may be shared, by whom, on which origin, and with whose credentials do connectors run from a shared artifact? Items: [F-t3-046](#F-t3-046), [F-t3-045](#F-t3-045), [F-t3-080](#F-t3-080), [F-t3-031](#F-t3-031). Constraints from the code: ZDR threads have no durable transcript, so sharing, replay and export must refuse them; grants belong in Control's resource taxonomy, never a Model Plane-local ACL; a dedicated user-content origin is a new attack surface. Recommendation: org-only default, viewer-bound connector calls, and MCP Apps on the existing sandboxed frame before any new origin.
7. **Learned and distributed behaviour: three-tier skill library, promotion gate and plugin supply chain.** Question: what are the vendor, org and user scopes, who approves promotion, and how are executable bundles pinned and killed? Items: [F-t3-007](#F-t3-007), [F-t3-058](#F-t3-058), [F-t3-059](#F-t3-059), [F-t3-014](#F-t3-014). The loop already enables machine-written skills at org scope with no status, inbox or eval, so the recommendation is to build the staging gate (owner scope, disabled by default) before enabling the loop for real orgs; note the gateway skill cache pulls once per org per TTL and would need a service-wide view for user-scoped skills.
8. **Model entitlements and provider credential ownership.** Question: does Control Plane own allowed_models, and where do bring-your-own (BYO) keys live? Items: [F-t3-023](#F-t3-023), [F-t3-065](#F-t3-065), [F-t3-042](#F-t3-042). model-gateway refuses the OrgPolicy allowed_models field because no service owns it. CLAUDE.md:23 assigns entitlements to Control Plane, so the recommendation is Control decides, Model Plane enforces at admission (fail closed, not the fail-open org_quota behaviour at sse.rs:1154), and BYO keys stay in Integration Core custody with per-connection ZDR and residency attestation that an admin form cannot assert.
9. **LLM reviewers on the permission path (HAP WS-3, ownership-matrix rule 12).** Question: is a tighten-only model reviewer ever acceptable? Item: [F-t3-097](#F-t3-097). A standing decision rules out an LLM on the permission hot path and rule 12 says Model Plane never invents approval authority. Only a design that can move Allow to Ask or Deny, never grant, and fails closed to a human card is compatible; start in shadow mode via the hook engine if at all.
10. **Connector content in shared Spaces and ZDR propagation for connector and web reads.** Question: how is personal mailbox and drive content read in a Space thread kept from other members, and how does ZDR reach connector reads, citations and research ledgers? Items: [F-t3-010](#F-t3-010), [F-t3-056](#F-t3-056), [F-t3-027](#F-t3-027), [F-t3-055](#F-t3-055). The code shows that non-ZDR runs persist truncated tool output into step records, so connector redaction (plan F8) must land before connector reads are proven live, and web policy must be enforced in both the inline loop and execution-core's web path.

### 9.4 Dependency notes

1. **The durable loop is the trunk.** [F-t3-002](#F-t3-002) needs [F-t3-016](#F-t3-016) and the OD8 renewable bearer, and in turn gates [F-t3-011](#F-t3-011), [F-t3-018](#F-t3-018), [F-t3-037](#F-t3-037), [F-t3-047](#F-t3-047), [F-t3-026](#F-t3-026), [F-t3-036](#F-t3-036), [F-t3-064](#F-t3-064) and [F-t3-083](#F-t3-083). Verification says the loop cannot continue after an approval today and that the continuation stub resumes only provider actions, book_shipment and tickets.create; split the item so the checkpoint-and-resume core ships first.
2. **Declared tool risk before anything MCP.** [F-t3-003](#F-t3-003) precedes [F-t3-006](#F-t3-006), [F-t3-010](#F-t3-010), [F-t3-025](#F-t3-025), [F-t3-030](#F-t3-030), [F-t3-045](#F-t3-045), [F-t3-057](#F-t3-057), [F-t3-059](#F-t3-059) and [F-t3-070](#F-t3-070), because both loops refuse MCP execution until capability-core provides a durable per-tool binding and the binding may never be synthesised from a model-supplied name.
3. **Context before cache before personalisation.** [F-t3-022](#F-t3-022) and [F-t3-020](#F-t3-020) feed [F-t3-001](#F-t3-001); [F-t3-001](#F-t3-001) feeds [F-t3-013](#F-t3-013), [F-t3-018](#F-t3-018), [F-t3-060](#F-t3-060) and [F-t3-061](#F-t3-061). Do runs restart from preamble plus goal today, so cross-run cache reuse is only the preamble and tools until [F-t3-001](#F-t3-001) lands.
4. **The computer track is strictly ordered.** [F-t3-008](#F-t3-008) then [F-t3-026](#F-t3-026); [F-t3-009](#F-t3-009) then [F-t3-035](#F-t3-035) then [F-t3-024](#F-t3-024); [F-t3-043](#F-t3-043), [F-t3-084](#F-t3-084), [F-t3-085](#F-t3-085) and [F-t3-087](#F-t3-087) all wait for [F-t3-009](#F-t3-009). Nearly all of the XL cost in [F-t3-024](#F-t3-024) sits in [F-t3-009](#F-t3-009) and must not be counted twice.
5. **The data-model track.** [F-t3-012](#F-t3-012) precedes [F-t3-013](#F-t3-013), [F-t3-015](#F-t3-015), [F-t3-031](#F-t3-031), [F-t3-046](#F-t3-046), [F-t3-052](#F-t3-052), [F-t3-069](#F-t3-069) and [F-t3-077](#F-t3-077). The full-text index must live in session-core (no cross-plane DB), exclude ZDR turns and honour the erase contract; the persisted message tree should follow the index, not lead it.
6. **Governance surfaces chain.** [F-t3-032](#F-t3-032) feeds [F-t3-043](#F-t3-043), [F-t3-055](#F-t3-055), [F-t3-059](#F-t3-059), [F-t3-063](#F-t3-063) and [F-t3-074](#F-t3-074); policy may only tighten relative to the server floor.
7. **Model record before provider breadth.** [F-t3-023](#F-t3-023) feeds [F-t3-042](#F-t3-042), which feeds [F-t3-028](#F-t3-028), [F-t3-040](#F-t3-040), [F-t3-041](#F-t3-041), [F-t3-065](#F-t3-065), [F-t3-072](#F-t3-072), [F-t3-073](#F-t3-073) and [F-t3-095](#F-t3-095); [F-t3-041](#F-t3-041) also needs [F-t3-029](#F-t3-029) and a Vertex attestation, and in turn gates [F-t3-051](#F-t3-051) and [F-t3-090](#F-t3-090).
8. **Attachments and Office.** [F-t3-017](#F-t3-017) precedes [F-t3-014](#F-t3-014), [F-t3-051](#F-t3-051), [F-t3-052](#F-t3-052), [F-t3-090](#F-t3-090) and [F-t3-091](#F-t3-091); [F-t3-014](#F-t3-014) precedes [F-t3-038](#F-t3-038) and [F-t3-058](#F-t3-058).
9. **Deploy before you build on it.** [F-t3-039](#F-t3-039) is a dependency of [F-t3-004](#F-t3-004) and [F-t3-079](#F-t3-079); [F-t3-005](#F-t3-005) and [F-t3-004](#F-t3-004) are prerequisites of [F-t3-062](#F-t3-062) and [F-t3-068](#F-t3-068).
10. **Demoted items should still be slotted.** [F-t3-016](#F-t3-016) was demoted because drain and Redis resume already ship and approvals already persist, but it remains a listed dependency of [F-t3-002](#F-t3-002), [F-t3-011](#F-t3-011), [F-t3-012](#F-t3-012), [F-t3-015](#F-t3-015) and [F-t3-064](#F-t3-064); treat its remaining M slice (shared cancel and queue state) as W1 work.

## 10. Risks and open questions

### 10.1 Main risks

1. **Plane-boundary erosion.** A thread-scoped sandbox workspace, staged attachments and connector content in Spaces all tempt shortcuts across planes. Verification found concrete cases: the audit hash chain must be delivered through the outbox and NATS and never touch audit-core's database ([F-t3-004](#F-t3-004)); the full-text index must sit in session-core, not Data Plane v2 ([F-t3-012](#F-t3-012)); grants for snapshots belong in Control's resource taxonomy ([F-t3-046](#F-t3-046)); no single Model Plane resolver can see every policy layer, so each owner plane must expose its own value and origin ([F-t3-068](#F-t3-068)). Also [F-t3-009](#F-t3-009), [F-t3-017](#F-t3-017), [F-t3-010](#F-t3-010), [F-t3-057](#F-t3-057).
2. **ZDR propagation into new stores.** Loop checkpoints, the search index, artifact versions, request capture, recordings, push payloads, terminal output, goal rows and telemetry exports are new places content could persist. The code already refuses several of these under ZDR (continuation, durable upload, artifacts, handles), so each item must carry an explicit ZDR clause and the resolver's golden matrix should gain a case per store. Items: [F-t3-002](#F-t3-002), [F-t3-012](#F-t3-012), [F-t3-015](#F-t3-015), [F-t3-031](#F-t3-031), [F-t3-034](#F-t3-034), [F-t3-035](#F-t3-035), [F-t3-037](#F-t3-037), [F-t3-046](#F-t3-046), [F-t3-050](#F-t3-050), [F-t3-071](#F-t3-071), [F-t3-075](#F-t3-075). Notification-core's README says the full payload still reaches Novu, so attention notifications must be content-free on all threads ([F-t3-015](#F-t3-015)).
3. **Authority creep.** Scoped approvals, stored consent for unattended runs, takeover carry-over, persistent named agents, viewer-bound artifact connectors and hooks each widen who or what can act. They must be tighten-by-default, expiring and audited; the Watch primitive deliberately refuses org-wide subscriptions and a stored record is not authority ([F-t3-047](#F-t3-047)). Items: [F-t3-006](#F-t3-006), [F-t3-018](#F-t3-018), [F-t3-019](#F-t3-019), [F-t3-025](#F-t3-025), [F-t3-032](#F-t3-032), [F-t3-045](#F-t3-045), [F-t3-063](#F-t3-063).
4. **A larger prompt-injection surface.** Browser pages, mail bodies, recall snippets, MCP tools, widgets, teach-mode recordings and imported memory feed untrusted text into the loop. The semantic screening pass is chat-only and shadow-only today, and two scanner copies are kept in sync by hand ([F-t3-033](#F-t3-033)). [F-t3-033](#F-t3-033) should land before or with [F-t3-008](#F-t3-008), [F-t3-010](#F-t3-010), [F-t3-003](#F-t3-003), [F-t3-044](#F-t3-044) and [F-t3-054](#F-t3-054).
5. **Cost growth without measurement.** More context per Do run, two-pass research, wide fan-out, image loops and VMs raise spend while the cache hit rate is unmeasured, image generation is not in the cost ledger and Gemini turns would be unmetered without pricing. [F-t3-022](#F-t3-022) and [F-t3-028](#F-t3-028) come first; see also [F-t3-036](#F-t3-036), [F-t3-037](#F-t3-037), [F-t3-041](#F-t3-041), [F-t3-051](#F-t3-051), [F-t3-092](#F-t3-092).
6. **Concentrated dependencies.** A third of the roadmap hangs on [F-t3-002](#F-t3-002), [F-t3-003](#F-t3-003) and [F-t3-009](#F-t3-009), and the renewable delegated credential (OD8) is a hidden prerequisite for [F-t3-002](#F-t3-002), [F-t3-018](#F-t3-018), [F-t3-036](#F-t3-036) and [F-t3-037](#F-t3-037). A slip there delays everything downstream.
7. **Effort was systematically underestimated.** Verification changed 40 effort estimates, 30 of them upward (15 from L to XL; the 11 in Priority and Recommended are [F-t3-008](#F-t3-008), [F-t3-014](#F-t3-014), [F-t3-027](#F-t3-027), [F-t3-032](#F-t3-032), [F-t3-037](#F-t3-037), [F-t3-043](#F-t3-043), [F-t3-046](#F-t3-046), [F-t3-047](#F-t3-047), [F-t3-051](#F-t3-051), [F-t3-058](#F-t3-058), [F-t3-062](#F-t3-062)) mostly because items bundle separable deliverables. Plan each XL as the split the verification proposes, not as one epic.
8. **Operational maturity.** The audit trail went undelivered with a fix built but not deployed, the ZDR unblock waits on an operator and commercial attestation, and no signed release candidate or rehearsed rollback exists. The evidence register for release tooling is dated 2026-08-17 and the conformance script reads a markdown table row, so state must be re-verified live ([F-t3-039](#F-t3-039), [F-t3-004](#F-t3-004), [F-t3-005](#F-t3-005)).
9. **Design conflicts inside the repo.** Several items contradict a documented ruling: queued-input classification is deliberately left to the model ([F-t3-016](#F-t3-016)); persisted message versions were rejected once ([F-t3-012](#F-t3-012)); the SandboxBackend trait was rejected until a second backend exists ([F-t3-009](#F-t3-009)); subscription fallback clashes with ownership-matrix rule 17 ([F-t3-072](#F-t3-072)); the @ trigger is reserved for Space-governed agent mentions ([F-t3-052](#F-t3-052)). Each needs an explicit reversal, not a silent change.
10. **Supply chain for executable extensions.** SKILL.md bundles, plugins and per-Space images bring tenant-authored and third-party code into the sandbox. Bundle scripts may run only inside the sandbox bound to the low-risk cap.command.sandbox, stdio and binary plugin components must be refused and reported, and install must list every component ([F-t3-014](#F-t3-014), [F-t3-059](#F-t3-059), [F-t3-084](#F-t3-084)).

### 10.2 Open questions

1. OD19: are Spaces the chat project container, what is the instruction precedence (org, Space, project, persona), what are the branch and rewind semantics, what is the share model, and who may author slash templates? ([F-t3-013](#F-t3-013), [F-t3-001](#F-t3-001), [F-t3-012](#F-t3-012), [F-t3-046](#F-t3-046), [F-t3-052](#F-t3-052))
2. OD13: who owns the agent-browser domain fence and per-site read/act consent: broker, Control or Quarry? ([F-t3-008](#F-t3-008))
3. OD8: how are delegated credentials renewed for runs that pause for hours or start unattended, without a service-principal shortcut? ([F-t3-002](#F-t3-002), [F-t3-018](#F-t3-018), [F-t3-037](#F-t3-037))
4. Owner ruling 2026-09-28: under what stored-consent contract may scheduled and triggered runs perform writes, and does the contract live in the Control-committed template as verification suggests? ([F-t3-018](#F-t3-018), [F-t3-047](#F-t3-047))
5. OD6: how does a plan continue after an approval pause: loop-level checkpoints (as the references do) or more action kinds in the continuation worker? ([F-t3-002](#F-t3-002), [F-t3-011](#F-t3-011))
6. Learning: what are the three skill tiers, should learned skills auto-accept at user scope once an eval passes, and which runs are reviewed? ([F-t3-007](#F-t3-007), [F-t3-058](#F-t3-058))
7. ZDR: which provider surface is attested first, Azure OpenAI or Azure AI Foundry, and when does Z-40 close commercially? Verification notes the two need separate attestations and that the Z-60 live proof runs first on a ZDR-TEST org with nothing attested, so a real-provider pass is a second step. ([F-t3-005](#F-t3-005))
8. Sandbox computer: self-hosted microVMs or an attested managed provider, which region and cost model, and are workspaces scoped to the thread, the Space or both? ([F-t3-009](#F-t3-009))
9. Computer use: is user-desktop control permanently out of scope or only parked? This decides whether the Endpoint Plane ever joins the agent path. ([F-t3-024](#F-t3-024))
10. OD7: what spend threshold triggers a confirm gate for wide research and other fan-out, given the plan's "no confirm gate in v1"? ([F-t3-036](#F-t3-036))
11. F12: should Ask mode read connected apps, as the references do, or stay Do-only? Connector reads in non-ZDR runs persist truncated output into step records, so the answer depends on connector redaction landing first. ([F-t3-010](#F-t3-010))
12. OD2: when is Do mode enabled on the product route? File tools, MCP and connectors in Do depend on it. ([F-t3-030](#F-t3-030), [F-t3-003](#F-t3-003))
13. Gemini: can Vertex AI in an EU region meet the residency and ZDR attestation bar? ([F-t3-041](#F-t3-041), [F-t3-051](#F-t3-051), [F-t3-090](#F-t3-090))
14. Entitlements and BYO: does Control Plane own allowed_models, and how are customer-billed BYO connections metered and attested? ([F-t3-023](#F-t3-023), [F-t3-065](#F-t3-065))
15. Effort in Do runs: the agent loop deliberately disables thinking (apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs:1336-1339), so extending the effort ladder to Do runs is a product decision, not a bug fix. ([F-t3-029](#F-t3-029))
16. Channel Plane: when does it move from docs-only to build? Outward channel adapters and channel-origin runs through the Agent API wait on it. ([F-t3-089](#F-t3-089), [F-t3-064](#F-t3-064))
17. HAP WS-3 and rule 12: is a tighten-only LLM reviewer ever acceptable on the permission path? ([F-t3-097](#F-t3-097))

### 10.3 Evidence limits that matter for decisions

- The four web products (claude.ai, ChatGPT, Perplexity, manus.im) rest on public documentation only [DOC]. Where a best practice comes solely from them, such as the per-conversation VM behind [F-t3-009](#F-t3-009), model entitlements behind [F-t3-023](#F-t3-023), memory controls behind [F-t3-020](#F-t3-020), deep research with an editable plan behind [F-t3-027](#F-t3-027) and the Compliance API behind [F-t3-004](#F-t3-004), the mechanism is not verified and only the product shape should be copied.
- Claude desktop's instruction scopes come from i18n label strings, not runtime assembly code, and the only documented precedence is that organisation instructions override personal preferences; the "org > Space > persona > user" ordering is Verevon's proposed design and must not be attributed to a reference [REA, corrected]. The reference check for [F-t3-001](#F-t3-001) also marks "one assembler used by chat and agents" and "every reference agent run inherits the conversation" as inference [INF].
- Fourteen items had a wrong prevalence count before correction; where the corrected count is not stated, this document avoids "n of N references" claims. The data-connected dashboard half of [F-t3-091](#F-t3-091) rests on one of twelve sources and documentation only [DOC].
- Several Verevon "built" states cannot be proven from code: whether the audit outbox fix is deployed ([F-t3-004](#F-t3-004)), whether the signed release candidate exists ([F-t3-039](#F-t3-039), register dated 2026-08-17), and whether Dragonfly or Redis is present in every deployment ([F-t3-016](#F-t3-016)). These are operator facts and should be checked live before planning.
- Some mechanism claims are inference from naming or field lists rather than observed behaviour: Cursor's "hydration flags" on RequestContext [INF], the SPA's browser-local transcript snapshot inferred from history.rs naming ([F-t3-021](#F-t3-021)) [INF], and the assumption that Responses continuation applies only to platform OpenAI or Azure routes ([F-t3-095](#F-t3-095)) [INF].
- Demand evidence is thin for the sandbox-extension items ([F-t3-084](#F-t3-084), [F-t3-085](#F-t3-085)) and for code-edit quality ([F-t3-088](#F-t3-088)); they were demoted on that basis and should not be scheduled without a concrete user need.
