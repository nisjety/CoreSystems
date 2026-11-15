## 2. What every reference shares

The comparison found 58 capabilities that are table-stakes across the 12 references: each is present in at least 4 of the sources that could have it. Verevon already matches the references on the chat substrate: streaming, rendering, web search, context compaction, subagents, a typed client contract and identity are shipped. The shared capabilities it lacks or only half has cluster in four places: continuity of the agent loop (run context, plan approval, scheduled runs), the conversation data model (versions, fork, search, projects, sharing), extensibility that actually executes (MCP tools, skill bundles, plugins, connectors) and governance surfaces (model entitlements, policy registry, memory controls, browser site policy). Desktop computer use is the one table-stakes capability that needs a new foundation.

Prevalence is written "n of N references". N is 12 for most categories; where a category could only be assessed on fewer sources the row says so (11 for multimodal and research, 9 for coding). Verevon status comes from the verified recommendation where one exists; where verification corrected a count, the corrected count is used and the row says "corrected".

### 2.1 Chat experience

| Capability | References that have it | Verevon today | Recommendation |
|---|---|---|---|
| Streaming transport and incremental conversation-state sync | 4 of 12 | Shipped | none needed |
| Rich Markdown, math, diagram and safe rendering stack | 6 of 12 | Shipped | none needed |
| Message actions: edit, retry, regenerate with model or effort switch, persisted versions | 4 of 12 | Partial | [F-t3-012](#F-t3-012) |
| Branch or fork a conversation from any message, with lineage | 8 of 12 | Partial | [F-t3-012](#F-t3-012) |
| Rich composer: typed inline tokens, @-mentions and slash menu | 6 of 12 | Partial | [F-t3-052](#F-t3-052) |
| Conversation organisation: pins, stars, archive, unread, sections, snooze, bulk | 9 of 12 | Partial | [F-t3-012](#F-t3-012) |
| Conversation search and agent recall of past chats | Disputed; counts conflict across clusters, so no count is stated | Partial | [F-t3-012](#F-t3-012) |
| Projects or workspaces with instructions, knowledge, members and retrieval auto-switch | 6 of 12 (corrected) | Partial | [F-t3-013](#F-t3-013) |
| Sharing: links, snapshots, replay and admin policy | 8 of 12 | Partial (share link absent) | [F-t3-046](#F-t3-046) |

The shared baseline is a durable message tree that users can edit, regenerate, fork and search, organised into projects and shareable by link. Fork from any message is in 8 of 12 references and conversation organisation in 9 of 12, which makes the history model the most widely shared chat capability [SRC/DOC]. Verevon streams and renders at parity, but its history foundation is thin: session-core has thread list, archive and presentation fields but no message parent or sibling, no fork or supersede route, no full-text index and no recall tool (apps/Model Plane/rust/services/model-gateway/src/http_routes.rs:137-185; apps/Model Plane/rust/services/session-core/migrations/0021_thread_presentation.sql:1-12). Chat projects are a browser-only folder list capped at 100 (apps/Frontend Plane/verevonv3/src/features/chat/lib/chat-organization.ts:2-34), while the real project container, Spaces, is not reachable from /chat. Verification notes that persisted versions with a model or effort switch are only 4 of 12 and DOC-only for the model-switch part, so the message tree matters more than the regenerate menu [DOC].

### 2.2 Multimodal & creation

| Capability | References that have it | Verevon today | Recommendation |
|---|---|---|---|
| Multi-format file attachments with published limits and out-of-context processing | 4 of 11 | Partial | [F-t3-017](#F-t3-017) |
| Streaming dictation into the composer | 6 of 11 | Partial (dictation per utterance, no live partials) | [F-t3-082](#F-t3-082) |
| Image generation with model choice and typed results | 4 of 11 | Partial | [F-t3-051](#F-t3-051) |
| Office document creation through skills and a sandbox, with verification | 5 of 11 | Partial | [F-t3-014](#F-t3-014) |
| Sandboxed data analysis on uploaded files | 4 of 11 | Partial | [F-t3-017](#F-t3-017) |

The baseline is a composer that accepts large files of many types, a sandbox that can open those files, and document output that is checked before delivery. Verevon caps attachments at 1 MB, 60,000 characters per file and 120,000 per turn and inlines extracted text into the message, so uploaded files never reach the code interpreter (apps/Frontend Plane/verevonv3/src/features/chat/lib/prepare-chat-attachments.ts:6-43). Office files already come out of a bubblewrap sandbox whose image carries openpyxl, python-docx, reportlab and pypdf (apps/Model Plane/rust/services/execution-core/src/code_interpreter.rs:1-45), but there is no skill bundle and no render-and-look check. Dictation is ahead of the original assessment: a speech-to-text call followed by a fail-soft cleanup pass is shipped (apps/Frontend Plane/verevonv3/src/shared/api/audio-client.ts:34-100); only streaming partials are missing. Image generation is a request flag that routes the whole turn to one hard-coded 1024x1024 call, not a tool an agent can use (apps/Model Plane/rust/services/model-gateway/src/sse.rs:1415-1440). The claude.ai limits conflict between two help articles (500 MB per chat file versus 30 MB in the sandbox article), so no single peer limit should be copied [DOC].

### 2.3 Research & web

| Capability | References that have it | Verevon today | Recommendation |
|---|---|---|---|
| Web search tool with grounded, cited answers | 11 of 11 | Shipped | none needed |
| Connected apps in chat (OAuth connectors with read and write actions, multi-account, @-mention scoping) | 5 of 11 (corrected) | Partial | [F-t3-010](#F-t3-010) |

Cited web search is the only capability every reference has, and Verevon ships it. Connected apps are the gap: the Ingestion side dispatches mail, drive and calendar reads end to end, but execution-core's catalogue omits the single-item and file-content actions and plain chat has no connector reads (apps/Model Plane/rust/services/execution-core/src/integration_tools.rs:1-160). Verification dropped Antigravity from the count because its evidence is a generic workspace API step, and notes that the ChatGPT read-versus-write gating is DOC-only and could not be re-fetched [DOC].

### 2.4 Agent runtime

| Capability | References that have it | Verevon today | Recommendation |
|---|---|---|---|
| Mid-turn steering and message queueing | 8 of 12 | Partial | [F-t3-016](#F-t3-016) |
| Context compaction core (prune, summarise, provider-native) | 7 of 12 | Shipped | none needed |
| Plan mode with reviewable plan and approval gate | 9 of 12 | Partial | [F-t3-011](#F-t3-011) |
| Subagent delegation and lifecycle | 7 of 12 | Shipped | none needed |
| Scheduled agent runs (cron or RRULE) with result delivery | 10 of 12 (two vendors double-counted) | Partial | [F-t3-018](#F-t3-018) |
| Layered custom instructions (global, organisation, space, project) applied everywhere | 8 of 12 | Partial | [F-t3-001](#F-t3-001) |

The baseline agent loop takes a plan, pauses for approval, accepts steering mid-turn, compacts its own context, delegates to children and runs on a schedule. Verevon has every piece but the joins: approving a plan records a grant and does not start the executing run (apps/Frontend Plane/verevonv3/src/features/chat/components/use-chat-controller.ts:2030-2135); queued input is per-process memory (apps/Model Plane/rust/services/model-gateway/src/queued_input.rs:1-80); each scheduled step is one tool-less 1,024-token completion (apps/Model Plane/rust/services/execution-core/src/grpc.rs:498-530); and the agentic branch seeds a fresh history with only the goal, so org, Space and persona instructions reach Ask turns but not Do runs (apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs:1185-1226). Verification adds two cautions: no reference implements multi-replica run control, so that part of [F-t3-016](#F-t3-016) is Verevon design [INF]; and the scheduled-run count of 10 of 12 counts Manus and Anthropic twice [SRC/DOC].

### 2.5 Tools, plugins & extensibility

| Capability | References that have it | Verevon today | Recommendation |
|---|---|---|---|
| MCP client runtime: transports, per-server and per-tool config, supervision and recovery | 6 of 12 | Partial | [F-t3-003](#F-t3-003) |
| Plugin package: one installable bundle of skills, connectors, commands, agents, hooks and templates | 7 of 12 | Stub | [F-t3-059](#F-t3-059) |
| Skill packages (SKILL.md open standard) with progressive disclosure | 9 of 12 | Partial | [F-t3-014](#F-t3-014) |
| Composer slash commands and @ or $ mentions | 9 of 12 | Shipped | none needed |
| Public API, SDKs and programmatic control of the same agent | 10 of 12 | Shipped | none needed |

MCP is the Model Context Protocol, the open standard for connecting tools to a model. The baseline is that MCP tools, skill folders and plugin bundles can all be installed and then actually run. Verevon registers MCP servers and offers their tools to Do runs, but the signed gate returns no capability id for dynamic MCP names and the inline Ask path refuses them by design, so no MCP tool can execute (apps/Model Plane/rust/services/execution-core/src/permission/mod.rs:200-293; apps/Model Plane/rust/services/model-gateway/src/tool_loop.rs:625-642). The plugin registry stores manifest JSON that nothing reads (apps/Model Plane/go/services/capability-core/internal/api/plugins.go:13-22). Skills exist as text records with matching and sharing, not as SKILL.md bundles with scripts. Verification found that no reference drives approval policy from a declared side-effect class; Manus declares five levels only for its browser tools and uses them for replay dedupe, so that design in [F-t3-003](#F-t3-003) is a Verevon synthesis [SRC].

### 2.6 Computer & environment control

| Capability | References that have it | Verevon today | Recommendation |
|---|---|---|---|
| Git worktrees for parallel agents | 10 of 12 | Absent (not applicable: not a coding product) | none needed |
| Desktop computer use (screenshots plus accessibility tree, mouse and keyboard on any app) | 8 of 12 | Absent | [F-t3-024](#F-t3-024) |

Computer use is the clearest table-stakes gap: the Do catalogue offers only browser.observe, browser.act and code_interpreter, the execution substrate is a one-shot bubblewrap child, and "desktop" is rejected as a broker grant (apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs:4190-4222). The owner parked user-desktop control, so the recommendation scopes computer use to the agent's own sandbox desktop and depends on the per-thread sandbox computer in [F-t3-009](#F-t3-009). Git worktrees are IDE table-stakes and are deliberately not adopted.

### 2.7 Coding capability

| Capability | References that have it | Verevon today | Recommendation |
|---|---|---|---|
| Diff review surface with scopes and accept or reject | 5 of 9 | Partial | [F-t3-076](#F-t3-076) |

Verevon renders read-only unified diffs and has an artifact version stepper, and the sandbox lease already computes a changed-file list at release, but nothing shows it to the user and no caller promotes an overlay into the Space (apps/Model Plane/rust/services/execution-core/src/workspace_hydrate.rs:102-125). Accept or reject by file or hunk is the nice-to-have step on top.

### 2.8 Memory, learning & personalization

| Capability | References that have it | Verevon today | Recommendation |
|---|---|---|---|
| Account long-term memory with user view, edit, delete, reset and pause controls | 8 of 12 (corrected) | Partial | [F-t3-020](#F-t3-020) |
| First-run onboarding that personalises the agent | 10 of 12 | Partial | [F-t3-061](#F-t3-061) |

The baseline is memory the user can see and govern, seeded by an onboarding step that asks who they are. Verevon lists, corrects and deletes memories (apps/Frontend Plane/verevonv3/src/features/settings/components/MemorySection.tsx:1-80) and its Dreaming loop already infers user-scope memories every 300 s (apps/Model Plane/rust/services/session-core/src/dreaming.rs:126-215), which puts it ahead of several references on extraction. It lacks an on, paused or off state, a reset-all and any org memory policy. Verification removed Cursor from the memory count and found a distinct pause control only in Claude, so pause is not table-stakes [SRC/DOC]. The onboarding wizard that exists is organisational (website crawl, connect accounts), not personal.

### 2.9 Models & routing

| Capability | References that have it | Verevon today | Recommendation |
|---|---|---|---|
| Plan, role and org model entitlements with admin defaults and caps | 4 of 12 (corrected; common rather than table-stakes) | Absent | [F-t3-023](#F-t3-023) |
| Per-model reasoning-effort ladder mapped to each wire vocabulary | 6 of 12 | Partial | [F-t3-029](#F-t3-029) |
| Provider-neutral inference layer: adapters, middleware chain and canonical event stream | 5 of 12 | Shipped | none needed |
| Bring-your-own keys, cloud accounts and customer LLM gateway | 6 of 12 | Partial | [F-t3-065](#F-t3-065) |
| Provider prompt-cache management: stability tiers, rolling breakpoints, lineage-scoped cache keys | 5 of 12 | Partial | [F-t3-022](#F-t3-022) |
| Frontier vendor breadth: Gemini, native DeepSeek, open-weight and specialist models | 5 of 12 | Absent for Gemini (DeepSeek only as an Azure MaaS id) | [F-t3-041](#F-t3-041) |

Verevon's inference layer is at parity in shape, but thin in reach. The model family enum has only OpenAiCompatible and Anthropic, so no Gemini or Vertex path exists (apps/Model Plane/rust/services/inference-core/src/provider/mod.rs:462-478). Effort is a three-value profile that maps to an Anthropic thinking budget and a Codex reasoning effort, and the OpenAI and Azure builder sends no reasoning effort at all (apps/Model Plane/rust/services/model-gateway/src/thinking.rs:1-80; apps/Model Plane/rust/services/inference-core/src/provider/openai.rs:430-455). There is no per-org model allowlist because no service owns the OrgPolicy allowed_models field (apps/Model Plane/rust/services/model-gateway/src/grpc.rs:148-160). Provider registration is environment-only and the only per-user connection is the ChatGPT subscription route (apps/Model Plane/rust/services/inference-core/src/config.rs:200-335). Cache tokens are measured end to end; what is missing is a hit-rate view and the off-by-default stable-prefix order (apps/Model Plane/rust/services/model-gateway/src/prompt_context.rs:1-60). No reference exposes a hit-rate KPI either, so that part is a Verevon addition [SRC].

### 2.10 Transparency, observability & control

| Capability | References that have it | Verevon today | Recommendation |
|---|---|---|---|
| Inline tool-call cards and work log | 5 of 12 | Shipped | none needed |
| Usage allowance and usage history page | 7 of 12 | Partial | [F-t3-028](#F-t3-028) |

Tool-call cards are at parity. Usage is reported to admins through the cost dashboard, but a user cannot see how much of their allowance a run has used: the usage event is emitted once at end of turn with no cache-token fields and there is no running-spend event (apps/Model Plane/rust/services/model-gateway/src/sse_events.rs:268-286). Verevon's Trace tab and context inspector already exceed most references on what the model saw; the live meter is the missing table-stakes piece.

### 2.11 Safety, permissions & governance

| Capability | References that have it | Verevon today | Recommendation |
|---|---|---|---|
| Named permission modes or presets (ask, accept-edits, plan, auto, full access) | 8 of 12 | Partial | [F-t3-032](#F-t3-032) |
| Model-based auto-review of risky actions (approve for me, guardian) | 7 of 12 | Absent | [F-t3-097](#F-t3-097) |
| Agent-browser governance: site allow and block lists, per-site consent, JS policy, origin trust | 7 of 12 | Partial | [F-t3-008](#F-t3-008) |
| Prompt-injection defences (untrusted-content labelling, scanning, sanitisation, layered monitoring) | 8 of 12 | Partial | [F-t3-033](#F-t3-033) |
| Credential storage and secret references | 7 of 12 | Unknown (no baseline record; connector tokens live server-side) | [F-t3-008](#F-t3-008) |
| Admin agent-policy registry and organisation setting locks | 7 of 12 | Partial | [F-t3-032](#F-t3-032) |
| Identity, RBAC, SSO and SCIM and spend limits | 7 of 12 | Shipped | none needed |

The governance baseline is a small set of named modes an admin can lock, site-level browser policy, layered injection defences and an audit of agent actions. Verevon has the primitives: a server-floored approval posture per profile and an autonomy rung on plan approval (apps/Model Plane/rust/services/model-gateway/src/profile.rs:90-125), trust-class provenance and a fail-closed injection scan (apps/Model Plane/rust/services/model-gateway/src/moderation.rs:40-110), and a browser-broker grant with allowed domains and actions. What it lacks is the product layer: modes are not a user-facing concept, the injection scan is 16 lowercase substrings, and the gateway still sends a placeholder grant id so no hosted-browser AI run is authorised (apps/Frontend Plane/verevonv3/apps/gateway/src/domains/browser.rs:1195-1275). The model-based reviewer is absent and conflicts with the standing "no LLM on the permission hot path" decision; only a tighten-only design is compatible, which is why [F-t3-097](#F-t3-097) is nice-to-have despite 7 of 12 prevalence. Verification cautions that fail-closed denylists are not the shared practice: Antigravity's denylist fails closed by documentation while Claude's site-category lookup fails open on timeout [DOC/SRC].

### 2.12 UI/UX & product design

| Capability | References that have it | Verevon today | Recommendation |
|---|---|---|---|
| Agent-first app shell and information architecture | 6 of 12 (one vendor double-counted) | Partial | [F-t3-015](#F-t3-015) |
| Attention model: typed notifications, sounds and badges | 5 of 12 | Partial | [F-t3-015](#F-t3-015) |
| Theming and appearance customisation | 7 of 12 | Partial | [F-t3-098](#F-t3-098) |
| Responsive web, mobile apps and multi-platform reach | 6 of 12 (one vendor double-counted) | Partial | [F-t3-015](#F-t3-015) |
| Internationalisation and localisation discipline | 7 of 12 | Partial | [F-t3-066](#F-t3-066) |

The shared shell puts chats and agent runs in one attention-ordered list, tells the user when a run needs them, and reaches them on a phone. Verevon splits chat, the Agent Run Console and Spaces into separate destinations, and its run-watch consumer maps only run-completed and run-failed (apps/Model Plane/go/services/capability-core/internal/runwatch/consumer.go:81-100); there is no PWA manifest and no Web Push. Control Plane already persists fontSize and compactMode but the gateway navbar payload drops them (apps/Frontend Plane/verevonv3/apps/gateway/src/domains/navbar.rs:84-104). The most visible i18n debt is Norwegian-only approval buttons and plan labels in the chat (apps/Frontend Plane/verevonv3/src/features/chat/components/ChatMessages.tsx:1900-1930). Verification notes that the closed attention set in [F-t3-015](#F-t3-015) is a Verevon design: Claude's kinds are permission, idle and question, and Antigravity distinguishes only action-required from done [SRC].

### 2.13 Platform architecture

| Capability | References that have it | Verevon today | Recommendation |
|---|---|---|---|
| One agent runtime behind many entry surfaces (desktop, CLI, headless, daemon, SDK, API) | 6 of 12 | Partial | [F-t3-064](#F-t3-064) |
| Desktop shell process model (thin native shell plus supervised backend child) | 7 of 12 | Absent (not applicable: web SPA) | none needed |
| Multiple execution environments from one UI (cloud, local, SSH, WSL) | 7 of 12 | Absent (not applicable: cloud-only by design) | none needed |
| One typed RPC contract between every client and the runtime | 6 of 12 | Shipped | none needed |
| Explicit state layout and per-conversation working directory | 6 of 12 | Partial | [F-t3-009](#F-t3-009) |
| Desktop distribution, installers and auto-update | 8 of 12 | Absent (not applicable) | none needed |

Four of the platform table-stakes are desktop or IDE concerns that a cloud web product does not need. The two that apply are one runtime behind many surfaces and a per-conversation workspace. Model Plane already serves the whole run lifecycle over HTTP with an OpenAPI spec and generated SDKs, but only the browser gateway consumes it and no API key is exchanged for a model-gateway bearer (apps/Model Plane/rust/services/model-gateway/src/http_routes.rs:132-236). Sandbox-manager has a lease lifecycle and Space-scoped workspace hydration, yet execution runs as a bubblewrap process per call with no persistent per-thread workspace (apps/Model Plane/go/services/sandbox-manager/internal/lease/lease.go:1-20; apps/Model Plane/rust/services/execution-core/src/sandbox.rs:270-360). Verification found that the per-task computer count double-counts vendors; roughly five distinct vendors host one, and Firecracker is the only named backend among them [DOC/SRC].

## 3. Best of breed: who does each thing best

The table names the reference whose implementation is the strongest model for each capability area, after the verification corrections. REA means reverse-engineered from the shipped app; SRC, source or shipped files; DOC, public documentation; INF, inference.

| Capability area | Best reference | Why it is best | Recommendation |
|---|---|---|---|
| Run context seeding | Cursor [SRC] | A typed request context with hydration flags and rule activation modes (always, glob, agent-fetched, manual) confirmed in source [SRC] | [F-t3-001](#F-t3-001) |
| Checkpointed resume | DeepSeek harness [SRC] | Checkpoints before model requests, tool effects and next steps, and cold resume repairs the log tail; crash-durable approval pause is not shown by any reference [SRC] | [F-t3-002](#F-t3-002) |
| MCP client runtime | Hermes [SRC] | Supervised servers with recovery and per-server trust tiers; the readOnlyHint check is binary and per server, not per tool [SRC] | [F-t3-003](#F-t3-003) |
| Per-tool connector policy | Claude desktop [SRC] | Per-tool toolPolicy with code-defined ask defaults and a never-allow set of 8 tools on each built-in connector [SRC] | [F-t3-003](#F-t3-003) |
| Audit and compliance | ChatGPT [DOC], Perplexity [DOC] | Compliance log with a scoped API, and audit JSON streamed by webhook to a SIEM; the ChatGPT page could not be re-verified [DOC] | [F-t3-004](#F-t3-004) |
| Approval decisions | Codex [SRC] with Claude [SRC] | Typed decisions (approved, for session, with amendment, denied, abort) and 55 versioned card templates; Claude adds 33 cause codes and an anti-decoy card [SRC] | [F-t3-006](#F-t3-006) |
| Learning governance | Manus [SRC], Hermes [SRC] | Learning proposals the user accepts or rejects (Manus); opt-in write-approval staging and a skill-mutation ledger with rollback; no reference evaluates learning outcomes [SRC] | [F-t3-007](#F-t3-007) |
| Agent browser vocabulary | Antigravity [SRC], Manus [SRC] | Widest action set with per-action state diffs and a recorded browser subagent; a 22-tool snapshot and ref contract [SRC] | [F-t3-008](#F-t3-008) |
| Browser site governance | Claude [SRC], Perplexity Comet [DOC] | Per-site consent with admin allow and block lists; Browser Control, Read Only or No Access per domain set by org admins [SRC/DOC] | [F-t3-008](#F-t3-008) |
| Per-task computer | claude.ai, Perplexity [DOC]; Cursor [SRC strings] | One isolated machine per task with snapshot and expiry; Firecracker is the only backend named [DOC/SRC] | [F-t3-009](#F-t3-009) |
| Connected apps | Manus [SRC]; Claude desktop [SRC] for policy | Per-task connector toggles and account selection; admin-managed built-in connectors with device-code sign-in and read-only switches [SRC] | [F-t3-010](#F-t3-010) |
| Plan content | Codex [SRC] | Decision-complete proposed plan after a read-only interview [SRC] | [F-t3-011](#F-t3-011) |
| Plan approval that resumes the run | DeepSeek harness [SRC] | Exit from plan mode is an ask_user wait with an Approve card, so approval resumes the same run [SRC] | [F-t3-011](#F-t3-011) |
| Clarifying questions | DeepSeek harness [SRC], Cursor [SRC] | Timed question card with drafts and late replies; trays for plan, questionnaire and queue docked above the prompt [SRC] | [F-t3-011](#F-t3-011) |
| History search and recall | Hermes [SRC] | FTS5 plus trigram index with BM25 and a recall tool; the only verified indexed search [SRC] | [F-t3-012](#F-t3-012) |
| Fork and message versions | Claude desktop [SRC]; ChatGPT, Perplexity [DOC] | Fork with lineage; the 1-of-N version tree with model or effort switch is documented only [SRC/DOC] | [F-t3-012](#F-t3-012) |
| Projects | Manus [SRC], claude.ai [DOC] | Project entity with instructions, files, members and roles; retrieval auto-switch with an indicator is Anthropic-only [SRC/DOC] | [F-t3-013](#F-t3-013) |
| Office deliverables | Claude desktop [SRC] | Four document skills ship scripts and three carry OOXML schema validators; SKILL.md files require render-and-look before delivery [SRC] | [F-t3-014](#F-t3-014) |
| Skill packages | claude.ai [DOC], Claude desktop [SRC] | Open SKILL.md standard with three-stage loading and executable bundles [DOC/SRC] | [F-t3-014](#F-t3-014) |
| Work list and notifications | T3 Code [SRC], Claude desktop [SRC] | Inbox-style sidebar with status pills and snooze; per-kind notification settings for permission, idle and question [SRC] | [F-t3-015](#F-t3-015) |
| Idempotent send receipts | T3 Code [SRC] | Serial command queue with durable idempotent receipts; no reference solves multi-replica run control [SRC] | [F-t3-016](#F-t3-016) |
| Attachments | claude.ai [DOC], DeepSeek harness [SRC] | Published limits and conversion funnel; a content-addressed object store under the harness home [DOC/SRC] | [F-t3-017](#F-t3-017) |
| Scheduled agents | Hermes [SRC] | Cron jobs are full agent runs with skills, context chaining and delivery; only dangerous commands are deny-by-default [SRC] | [F-t3-018](#F-t3-018) |
| Stored approvals for unattended runs | Claude desktop [SRC] | Scheduled tasks store approved permissions per tool with an admin-managed lifetime and a non-delegable tool set [SRC] | [F-t3-018](#F-t3-018) |
| Agent and automation hubs | Perplexity [DOC], Manus [SRC] | Hubs with Active, Paused and Completed states and Run now, Pause, Edit controls; named persistent agents [DOC/SRC] | [F-t3-019](#F-t3-019) |
| Memory controls | claude.ai [DOC] | View, edit, delete, reset and org policy; the separate pause control is Claude-only [DOC] | [F-t3-020](#F-t3-020) |
| Long transcripts | T3 Code [SRC], DeepSeek harness [SRC] | Virtualised timeline; trailing-block freeze with grammar-state resume for highlighting [SRC] | [F-t3-021](#F-t3-021) |
| Prompt cache | Hermes [SRC] | Prefix stability treated as an invariant, wire-only recall replayed byte-identically [SRC] | [F-t3-022](#F-t3-022) |
| Model entitlements | ChatGPT [DOC] | Plan, role and org allow-lists with admin defaults; additive roles plus Lockdown roles that override [DOC] | [F-t3-023](#F-t3-023) |
| Desktop computer use | Manus [SRC] | Accessibility-first engine: UI Automation tree with short handles, element-targeted actions, synthesised input only as fallback [SRC] | [F-t3-024](#F-t3-024) |
| Human takeover | Manus [SRC/DOC], T3 Code [SRC] | Agent-suggested takeover cards with hand-back; real human-versus-agent input arbitration [SRC] | [F-t3-026](#F-t3-026) |
| Deep research | ChatGPT [DOC] | Clarifying questions and an editable plan before the run [DOC] | [F-t3-027](#F-t3-027) |
| Cost transparency | Manus [SRC], Antigravity [SRC] | Live meter with pre-run estimate; cost by token category and quota display [SRC] | [F-t3-028](#F-t3-028) |
| Effort ladder | Hermes [SRC] | One canonical ladder clamped per model and mapped per wire format [SRC] | [F-t3-029](#F-t3-029) |
| Artifact canvas | Manus [SRC], claude.ai [DOC] | Server-side versions with restore and editor attribution; AI edits scoped to a selection [SRC/DOC] | [F-t3-031](#F-t3-031) |
| Policy registry and modes | Codex [SRC], Claude desktop [SRC], ChatGPT [DOC] | Typed mode vocabulary with per-effect approval config; schema-as-source registry with fail-closed values; Lockdown mode [SRC/DOC] | [F-t3-032](#F-t3-032) |
| Injection defences | Perplexity [DOC], Claude desktop [SRC] | Most layered documented stack; a homoglyph-aware scrubber that neutralises forged envelope tags [DOC/SRC] | [F-t3-033](#F-t3-033) |
| Run transparency | DeepSeek harness [SRC] | Trajectory ledger with exact request capture and a timing waterfall; typed end reasons and visible retries [SRC] | [F-t3-034](#F-t3-034), [F-t3-049](#F-t3-049) |
| Live computer panel | Manus [SRC], Codex [SRC] | A viewer per tool kind with picture-in-picture; a PTY exec contract with yield and poll [SRC] | [F-t3-035](#F-t3-035) |
| Wide research | Manus [SRC/DOC] | Parallel isolated sub-agents with an estimate, subtask grid and spend gate [SRC/DOC] | [F-t3-036](#F-t3-036) |
| Gemini capabilities | Antigravity [SRC], Perplexity [DOC] | Gemini thinking levels and typed media parts; the widest model line-up [SRC/DOC] | [F-t3-041](#F-t3-041) |
| Sandbox egress | claude.ai [DOC], Antigravity [SRC] | Four egress tiers with plan-dependent defaults; a TLS-intercepting proxy with fine-grained credential grants [DOC/SRC] | [F-t3-043](#F-t3-043) |
| MCP Apps and widgets | Codex [SRC] | Full ext-apps implementation with a gated JSON-RPC bridge and form elicitation [SRC] | [F-t3-045](#F-t3-045), [F-t3-044](#F-t3-044) |
| Sharing and replay | Manus [SRC] | Visibility, roles, replay and admin policy on one snapshot model [SRC] | [F-t3-046](#F-t3-046), [F-t3-050](#F-t3-050) |
| Event triggers | Cursor [SRC], Hermes [SRC] | Automations with event triggers, LLM filtering and dry runs; source-hash wake gates and suggestions capped at 5 [SRC] | [F-t3-047](#F-t3-047) |
| Readable long runs | T3 Code [SRC], DeepSeek harness [SRC] | Settled-turn folding; four density levels and a fixed activity vocabulary [SRC] | [F-t3-048](#F-t3-048) |
| Composer | T3 Code [SRC], Antigravity [SRC] | Atomic typed chips with per-thread drafts; typed scope items and server slash commands [SRC] | [F-t3-052](#F-t3-052) |
| Plugins | Claude desktop [SRC], Codex [SRC], Hermes [SRC] | Typed manifest with sha-pinned binaries; open manifest with foreign-namespace adapters; a catalogue pinned to commits with a kill list [SRC] | [F-t3-059](#F-t3-059) |
| Personalisation | Hermes [SRC], Claude desktop [SRC] | Frozen, bounded user snapshot in the prefix; onboarding that writes name, role and topics into instructions [SRC] | [F-t3-061](#F-t3-061) |
| Provider registry and BYO | Hermes [SRC], Claude desktop [SRC] | 41 overridable provider profiles; BYO cloud accounts with helper credentials that expire [SRC] | [F-t3-065](#F-t3-065) |
| i18n and UI gates | DeepSeek harness [SRC], Hermes [SRC] | Typed dictionaries with a gate against hard-coded copy; layered locale packs [SRC] | [F-t3-066](#F-t3-066) |
| Code mode and tool index | Hermes [SRC], Codex [SRC], DeepSeek harness [SRC] | Deferred tool search with core tools never deferred; model-written JavaScript in a V8 isolate; every sub-call re-enters the guarded pipeline [SRC] | [F-t3-070](#F-t3-070) |

No single reference is the model for the whole product. Claude desktop is the best overall model for governed autonomy and calm UX: typed approval contracts, stored permissions for unattended runs, per-site browser consent and document skills with validators, all of which map onto Verevon's signed-gate design [SRC]. Hermes is the model for the Model Plane internals that users never see but feel: prompt-cache stability, the effort ladder, indexed history recall, scheduled runs as full agent runs and governance of what the agent learns [SRC]. DeepSeek harness is the model for transparency and durability: checkpointed resume, the trajectory ledger, typed lifecycle events and plan approval that resumes the run [SRC]. Manus is the model for the computer-using half of the roadmap: sandbox computers, accessibility-first desktop control, takeover, replay, wide research and sharing [SRC/DOC]. T3 Code is the model for the chat shell itself: the inbox sidebar, virtualised timeline, composer chips and idempotent receipts [SRC]. Codex is the model for extensibility contracts: plugin manifests, MCP Apps, typed decisions and plan content [SRC]. Antigravity, Cursor and the web products contribute narrower patterns: the Gemini family and browser subagent, the typed run context and event triggers, and the documented enterprise controls for entitlements, compliance and deep research [SRC/DOC].
