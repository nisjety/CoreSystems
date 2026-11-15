# Verevon baseline: the decision register (adopted / scheduled / rejected / deferred)

Audit 3 (Model Plane + Verevon chat and agent pages), role **v-planned-decisions**. Read-only.

- **Repo:** `C:/dev/CoresSystem`, HEAD `afb83928` (2026-10-10 20:18 +0200). The working tree carries **311 uncommitted paths** (190 modified, 116 untracked, 5 deleted). The Oct 10-12 "build passes" (ZDR waiver, Quarry option B, rerank) live only in that dirty tree. In this file, "code" means the files on disk. Untracked or uncommitted items are flagged.
- **Provenance tags:** SRC = read directly in source, DOC = repo documentation, INF = my inference (basis given). No REA was used: the subject is first-party source.
- **Path shorthands:** `MP` = `apps/Model Plane`, `MG` = `MP/rust/services/model-gateway/src`, `EC` = `MP/rust/services/execution-core/src`, `SC` = `MP/rust/services/session-core/src`, `IC` = `MP/rust/services/inference-core/src`, `CT` = `MP/rust/crates/mp-contracts/src`, `CC` = `MP/go/services/capability-core`, `V3` = `apps/Frontend Plane/verevonv3`, `SPA` = `V3/src`, `GW` = `V3/apps/gateway/src`, `AUD` = `V3/apps/verevon-web/plans/system-audits`.
- Machine-readable twin: `v-planned-decisions.baseline.capabilities.json` (163 records: 68 shipped, 56 partial, 20 planned, 19 absent; most absent records are absent by decision).

## 0. Where the decisions actually live (read in this order)

Many of the docs named in the task brief are dated. The live decision record moved to two places.

| Source | Last change | Role | Trust |
|---|---|---|---|
| `MP/docs/decisions/ledger.md` (4,613 lines) | 2026-10-08 | **Canonical decision ledger**, with states proposed / implemented / deferred / rejected / archived. Adopted from DeepSeek's Agent Notes. | High. Entries are dated and evidence-bound. Newer entries (S3.3, S4.2, S4.3, ZDR) are appended below older ones, so it is not strictly newest-first. |
| `AUD/` (`PROGRAM_SUMMARY_AND_STATUS_2026-10-07`, `CHAT_PARITY_AUDIT_2026-10-01` + `.json`, `CHAT_GAP_CLOSURE_PLAN_2026-10-01` + `.json`, `BUILD_PASS_1..4_5` 2026-10-10..12, `LEARNING_*`, `CACHE_COMPACTION_AUDIT`, `chat-wave2-specs`) | 2026-10-12 | **Current program state**: what is proven live, built dark, blocked, and owner decisions OD1-OD20 | High (newest). Has its own second-reader verification. |
| `claude-hermes-deepseek.md` (CHD) | 2026-08-26 | Harness audit + 37-row feature-parity matrix vs Claude Code / DeepSeek / Hermes (+ pi) | Medium. Self-corrected several times; the matrix is closer to the truth than the prose. |
| `harness-adoption-execution-plan.md` (HAP) | 2026-08-25 | 25-item adoption plan, all items "resolved" 2026-08-24, plus six owner product decisions | Medium-high, for what it covers |
| `model-plane-to-verevon-parity.md` (PAR) | 2026-08-22 | Stream contract and the "gateway is a verbatim pipe" rule | Medium. Its gap rows were mostly closed by HAP WS-5. |
| `MP/docs/verevon_CHAT_PARITY_BACKLOG.md` (BKL) | 2026-08-09 | 26-item ranked backlog vs Claude Code, Codex, OpenCode, ChatGPT, Perplexity, Manus, Hermes, OpenClaw | Medium. Its item numbers are still cited by the 10-01 audit. |
| `V3/apps/verevon-web/plans/chat-workspace/VEREVON_CHAT_WORKSPACE_IMPLEMENTATION_PLAN.md` (CWP) | 2026-09-28 | Product + architecture decision (assistant-ui as blueprint, AG-UI adapter, A2A behind the UI) | Medium |
| `apps/VEREVON_UI_COWORK_RESEARCH_2026-08-13.md` (COW) | 2026-08-14 | Buzz / Grok Bot adopt and do-not-adopt list (Space-focused) | Medium |
| `apps/GRAPHITI_ADOPTION_ADR_2026-09-07.md` (GRA) | 2026-09-08 | ADR-0004: Graphiti not adopted | Proposed |
| `Verevon-ai-first-plan.md` (AIF) | 2026-08-26 | AI-first creed milestones M1-M4 (outward MCP/WebMCP) | Partly stale (see section 16) |
| `MP/MODEL_PLANE_ROADMAP.md`, `MODEL_PLANE_STATUS.md` | 2026-08-05 (evidence 2026-07-16) | Secure-MVP release program | **Stale** on runtime features. Still valid as the release-gate list. |
| `MP/docs/REFERENCE-MATRIX.md` | 2026-05-30 | Reference adoption tiers | **Very stale.** Nearly every "not started" is contradicted by code. |
| `MP/docs/MODEL_PLANE_V2_PARITY.md` | 2026-08-05 | v2 donor parity | Stale but directionally right (Gemini, Mistral, Ollama still missing) |
| `MP/docs/CAPABILITY_PROMOTION_LEDGER.tsv` | 2026-08-17 | Capability release state | All 28 rows are `source_only` |
| `OPTIMIZATION.md` | — | Control/Ingestion Go/Python perf checklist | Not relevant to chat or agents |

**Standing method rule the repo adopted after repeated wrong audits** (CHD TL;DR, LED 2026-08-25): a check mark means "the mechanism was built". It does not mean a user can reach it. Each claim has to be traced advertisement → dispatch → writer → reader. Of 22 parity claims, 8 were built but unreachable on 2026-08-25. All 8 were fixed 2026-08-26, and 2 of those fixes were themselves inert until corrected that day (LED, "The 8 reachability defects fixed"). The final audit should apply the same rule: **net-new vs already planned** must be judged against reachability, not against file existence.

**The route constraint that governs every "live" claim:** all live testing runs on the **ChatGPT "Terra" subscription route** (`openai-codex-subscription`, model `gpt-5.6-terra`, Codex app-server transport). It is never run on the paid "Verevon Balance" route (GCP section 2). Plan mode, wide research, image, deep research and skills are disabled on that route (CPA:131). As a result, many shipped mechanisms cannot be proven live. DOC.

---

## 1. Chat experience

**Adopted and in code**
| Item | Source idea | Status | Evidence |
|---|---|---|---|
| Typed SSE `ChatEvent` taxonomy (`result_receipt`, `reasoning_delta`, `citation`, `memory_recall`, `plan_proposed`, `queued_input`, `stopped` …); the gateway is a byte-verbatim pipe | PAR section 0 rule | shipped | SRC `MG/sse_events.rs:460-479`, `V3/apps/gateway/tests/sse_relay_no_allowlist.rs` |
| Conversation-node renderer registry + three-path equivalence gate (full load = rehydrate = live append) | DeepSeek `ConversationNodeDefinition` | shipped | SRC `SPA/shared/chat-nodes/registry.ts:41`, `derive.ts:81` |
| Tool presentation recomputed at render, never persisted; diff card | DeepSeek tool-presentation intent | shipped | SRC `SPA/shared/chat-nodes/tool-presentation.ts`, `diff.ts` |
| Keyed projection channel (key + stream generation) | DeepSeek keyed projection | shipped (readiness and gap-repair NOT adopted) | SRC `SPA/shared/projection/keyed-channel.ts:56` |
| Mid-run input as a **pause the model classifies** | pi `steer`/`followUp`, replaced by a third semantic (owner decision 2026-08-22) | shipped | SRC `MG/queued_input.rs:47,62,66`; `SPA/features/chat/components/ChatPage.tsx:253` |
| Distinct `stopped` vs `done`; continue after stop | — | shipped (live-proven per CPA section 4) | SRC `SPA/shared/api/chat-client.ts:1510` |
| Rich-event resume (frames + `id:` + `Last-Event-ID`) | DeepSeek | partial: in-process unless `REDIS_URL` is set (CPA:122) | SRC `MG/sse.rs:489` `emit_and_buffer`, `MG/stream_buffer.rs:82` |
| Artifacts with version history and a revision diff | Claude artifacts / canvas | shipped | SRC `MG/tool_loop.rs:2618`; `SPA/features/chat/components/ChatArtifactPanel.tsx` |
| Office export (docx / xlsx / pptx / print-PDF) | Copilot | shipped (no charts or images in exports) | SRC `SPA/features/chat/lib/chat-export/*.ts` |
| Regenerate / edit-resubmit with 1/N siblings | ChatGPT / Claude | partial: final exchange only, lost on reload | DOC `AUD/CHAT_PARITY_AUDIT_2026-10-01.md:123-124` |
| Thread `origin` dimension (chat list shows only chat threads) | chat-design invariant I2 | shipped | SRC `GW/domains/chat/history.rs:21` (`MAX_THREADS=80`) |

**Rejected / deferred / parked**
- **Session-as-tree / branching (pi):** HAP 4.5 status "decided as *wait*, pending a real need". Code has only a client-side "Fortsett i ny chat" (`SPA/features/chat/components/ChatMessages.tsx:807`). Proto `parent_message_id = 21` is unused (`MP/proto/model_plane/v1/gateway.proto:250`). CPA:107-112 records the durable fork as parked. SRC+DOC.
- **Share link (BKL #22):** Wave 6 deferred (I19). Recommended default (OD19): org-internal sign-in snapshot, 30-day, revocable, redacted. Not built (I searched SPA chat for share-link code; `GW/domains/shares.rs` is document sharing). SRC+DOC.
- **Checkpoint/rewind (BKL #7):** not built. Recommended scope (OD19): conversation + artifact versions, with effectful turns non-rewindable. SRC search + DOC.
- **Search over past chats (BKL #20, Hermes FTS5):** gap. Only a client-side title search exists (`SPA/features/core/components/CoreNavbar.tsx:397`). There is no `tsvector` in session-core. SRC.

## 2. Multimodal & creation

**Adopted / built:** speech TTS/STT (`IC/provider/speech.rs`, 1,073 lines; CHD corrected an earlier "voice ❌"), a governed push-to-talk Voice mode (`SPA/features/voice/VoiceModeDialog.tsx`), and realtime, video (Sora), doc-intel, translation and language providers (`IC/provider/*.rs`; V2P marks all of these "Partial"). Also image generation (`MG/sse.rs:1427`), vision (`IC/provider/vision.rs`), attachments with 1 MB / 60k-char caps (`GW/domains/chat/documents.rs`), and Azure fine-tuning jobs (`SC/finetune_grpc.rs`, `MG/finetune_azure.rs`; INF: not referenced by any harness decision doc). Maturity: **mostly built and unit-tested, not live-proven**. CPA:127/146/147 show no live run for image, vision or voice, and none of them runs on Terra.

**Decisions**
- **Full-duplex barge-in voice:** recommended **park** (OD19). Keep push-to-talk. ZDR voice is refused. BKL section 4 #2 also asked for a GTM call. DOC.
- **Voice huddles / realtime audio for collaboration:** **NOT adopted** (COW section 4 #6). DOC.
- **Office in-place edit, save to OneDrive/Drive, Sheets/Graph read-write, OCR:** scheduled in Wave 4/5 behind OD14/OD18 (CPA:63, :107). DOC.
- **Deck generation, charts:** partial. Only pptx export exists, and there is no native chart renderer (CPA:61, :91). DOC.

## 3. Research & web

**Adopted / built**
- web_search / fetch_url (`MG/tool_loop.rs:2874`, `:3024`). Live on Terra (PSS A6). SRC+DOC.
- Knowledge, graph, wiki and contradictions over the Data Plane (`MG/tool_loop.rs:3375`, `:3506`). BKL section 3 calls grounded retrieval "Verevon's moat". SRC.
- **Wide research fan-out (Manus style)**: `subagent.research_fan_out` (`EC/subagent/research_fan_out.rs:36,114`; dispatch `EC/runtime_loop/agent.rs:2331`). It is flag-gated. The route budget on Terra is 10 rounds / 2 subagents (`MG/sse.rs:820`, tests :854). A 3-way fan-out was live-proven on Terra 2026-10-01. Scale presets 12/24/48 wait on OD7. SRC+DOC.
- **Tool-argument grounding gate**, claimed as a lead over every reference harness (`CT/tool_arguments.rs:583,631`). Measured before the gate: 55-70% fabrication in the agent loop and 100% in chat (CHD TL;DR). SRC.
- Deep research with coverage notes (`MG/deep_research.rs`): live-proven, but not on Terra (CPA:145). Citations with claim ids: unit-tested only, and the client binds by array position (CPA:79).
- Connectors: Outlook/Gmail/OneDrive/Calendar are **listings only**, Do-mode only, and have never run against a real mailbox (`EC/integration_tools.rs`; CPA:92).

**Decisions**
- **Reranking belongs to the Data Plane** (ownership rule 15). Quarry's own LLM reranker is non-compliant. "Option B" (`POST /v1/rerank`, Quarry as a caller) was built dark in build passes 3-5 (uncommitted; `WEB_RERANK_ENABLED=0`). QB-T09, the Quarry client, is **do not ship** (BP 2026-10-11/12). DOC.
- **Owner rulings 2026-09-28:** personal connections are owner-only; mail-to-task uses Postmark self-forward; scheduled runs are draft-only (GCP section 2). DOC.
- **Graphiti: NOT adopted**, in any of its three shapes. Only validity intervals on `graph_claims` are taken, and automatic invalidation is explicitly not (ADR-0004, proposed). The binding constraint is extraction throughput: 12 documents announced, 2 extracted. DOC.

## 4. Agent runtime

**Adopted (harness ideas → code)**
| Idea | From | Status | Evidence |
|---|---|---|---|
| Parallel tool dispatch (3-phase, shared atomic subagent budget) in both loops | DeepSeek | shipped | SRC `EC/runtime_loop/agent.rs:1874`, `MG/tool_loop.rs:7156` |
| Tool retry gated on `is_risky_call` + drift contract | DeepSeek/all | shipped | SRC `EC/runtime_loop/retry.rs:150`, `MG/tool_retry.rs`, `MP/rust/services/model-gateway/tests/tool_retry_contract.rs` |
| Refuse only the truncated final call of a length-stopped message | pi (made more precise) | shipped | SRC `EC/runtime_loop/retry.rs:214` |
| Leaf/orchestrator split (depth 1 + risky refusal for children) | Hermes | shipped | SRC `EC/subagent/mod.rs:29` |
| Per-run child ceiling instead of worktrees | DeepSeek `maxTotalAgents` | shipped (default 8, org override, hard 100) | SRC `EC/subagent/mod.rs:63,71` |
| Durable lineage + cold resume (answer on the child run; consent-gated read) | DeepSeek continuation | shipped (staging pass requested) | SRC `EC/runtime_loop/agent.rs:4262`, `EC/permission/mod.rs:160` |
| Server-side plan-mode enforcement | Fix of Claude Code's prompt-only plan mode | shipped | SRC `EC/runtime_loop/agent.rs:1804` |
| Autonomy ladder per call with justification | DeepSeek SandboxMode, Claude Code compound approval shape | shipped (grant carrier in-memory) | SRC `CT/autonomy.rs:59,164`; `MP/rust/services/execution-core/tests/autonomy_ladder_contract.rs` |
| Tiered compaction in both loops + **native Anthropic context editing / compaction** | Claude Code tiers, Hermes native-first | shipped | SRC `MG/compaction.rs:145`, `EC/compaction_budget.rs`, `IC/provider/anthropic.rs:481,519` |
| Need-driven reattachment (`reattach_context`, `reattach_skill`) | Claude Code reattachment, reframed (owner decision #4) | shipped | SRC `MG/tool_loop.rs:3253,3319`, `MG/context_reattach.rs:87` |
| Compaction-recall eval harness | Hermes `evals/compaction` | shipped | SRC `MP/python/eval-lab-py/cases/compaction/13_*.yaml`, `.github/workflows/eval-lab.yml` |
| Two loops kept separate (HARN-1/2 rejected on measurement) | — | decision upheld | DOC `MP/docs/decisions/ledger.md:1410`, `MP/docs/postmortem/0001-*` |
| Durable process registry: `process_*` tools (S4.2) | Codex/Manus background terminals (INF) | shipped (source) | SRC `EC/runtime_loop/agent.rs:4089-4109`, `EC/process_host.rs` |
| Space Watch primitive (S4.3 steps 1-3) | — | partial | SRC `CC/internal/watch`, `CC/internal/processwatch`; DOC ledger:4494 |
| Approval continuation with AES-256-GCM descriptor | — | partial: the key is empty on the stack, so a pause returns `unavailable` (AIF Phase 0) | SRC `SC/continuation_crypto.rs:1` |
| Do mode on the subscription route | — | partial: dark by default | SRC `CT/agentic_subscription.rs:41` (`AGENTIC_SUBSCRIPTION_ENABLED` default false) |
| Plan proposal + editable plan gate | Manus Plan Mode | partial: the approved plan is consumed by no SPA turn (CPA section 2.2) | SRC `EC/runtime_loop/agent.rs:10998` (test), `SPA/shared/api/chat-client.ts:1911` |
| Scheduling | Manus / ChatGPT tasks | partial: a single no-tool call capped at 1,024 tokens, draft-only by ruling | SRC `EC/grpc.rs:522`; `SPA/features/settings/components/CronSchedulesSection.tsx` |
| Hooks Pre/PostToolUse | Claude Code | partial: not fed to agentic runs, in-memory | SRC `EC/hook/mod.rs:2` |

**Rejected / not adopted (with reason)**
- **Fork-semantics subagent:** REJECTED 2026-08-24, because DeepSeek rejected it ("inherited turns violate the fresh-context contract"). DOC ledger:1798. SRC: `run_subagent` builds a fresh `LoopContext`.
- **Git-worktree isolation:** replaced by the per-run child ceiling (ledger:1821).
- **DeepSeek FIFO inbox:** not adopted, because delegation is synchronous within a turn (HAP addendum).
- **"Fresh context" toggle in the approval control:** not adopted, because nothing iterative reads it (HAP 3.5).
- **Typed handoff report `{status, summary, evidence, nextSteps, blocker}`:** deferred. Trigger: the first code that branches on a delegation outcome (HAP 3.3).
- **Self-modification / agent-mounted plugins (DeepSeek cordis):** "needs a product call". The only reference implementation labels itself a non-security boundary (CHD matrix).
- **Model-authored orchestration scripts:** open. Temporal exists, but it is not the production path. "Whether we WANT model-authored fan-out is the prior question" (ledger 2026-08-26).
- **BKL section 4 #1 (multi-agent stance):** Cognition vs Anthropic. In practice, children get research tools only (GCP standing-rule 5). INF from the C8 design.

**Scheduled (not built):** code mode, i.e. tools callable from inside the sandbox (BKL #11, CPA:43); Manus-style todo recitation (BKL #10, CPA:110); a live credit meter; a 300 s bearer extension (OD8); `E12`, model-initiated `browser_agent` in Do runs (Wave 6).

## 5. Tools, plugins & extensibility

**Built:** a **single Capability registry with a `Kind` discriminator** (`CC/internal/registry`). CHD's 2026-08-22 correction explains why "plugin/MCP stub" rows were wrong. Also built:
- MCP client registry: 1,802-line `CC/internal/api/registry_apis.go`, SSRF ranges at :377, OAuth vault. It is bridged into the agent loop by `EC/mcp_gateway.rs` and has a Settings UI (`McpServersSection.tsx`).
- Plugins and Skills sections in Settings. Skills have a ~1% (8,000-char) budget with degrade-before-drop (`MG/skills.rs:775`, `EC/runtime_loop/skill_budget.rs:62`).
- A compositional system prompt (`EC/runtime_loop/agent.rs:208`, from pi).
- Built-in slash commands (`SPA/features/dashboard/home/DashboardComposer.tsx:540`).
- Persona and an authored-instruction hierarchy (ADR-0003, implemented 2026-08-20).
- A typed action registry with **8 model-executable actions** (`SPA/shared/actions/model-eligibility.ts:126`). SRC.

**Decisions**
- **`strict: true` function schemas:** DEFERRED. They disable parallel tool calls, gpt-5-mini lacks support, and min/max constraints are lost. Pre-dispatch validation was built instead (ledger:2038, :2103; `IC/provider/mod.rs:1278`). SRC+DOC.
- **AG-UI:** kept as an **intentionally unwired interop surface** (`SPA/shared/api/ag-ui-client.ts:1` "DO NOT clean this up"). The gateway route `GW/domains/ag_ui.rs:58` now sits behind `require_session`, which resolves CHD 11.4's "fix or delete". CWP treats AG-UI as a gateway interop adapter and **A2A** as behind-the-UI delegation (planned, design phase 9). SRC+DOC.
- **assistant-ui:** a blueprint only. Its React components are not installed (CWP section 2). DOC.
- **Outward MCP server + WebMCP:** planned for M4, after M2 (AIF Phase 4). Absent: I searched for `modelContext`/`registerTool` and found nothing, and `MP/bridges` has only the consumer-side `mcp-bridge` and `lsp-bridge`. SRC+DOC.
- **Hermes community governance** (install vs load split, hash-the-declared-set consent, scanning agent-authored items): recommended in CHD 13.4. Not built (INF, no code found in the paths read).
- **Tool-catalogue budget:** a recognised gap (about 16k unbounded chars vs the 8k skill cap). It is unbuilt, and it collides with the purpose-lock if done by selection (ledger Needle entry). SRC search: none.
- **Still open per CHD 13.3:** `context: 'fork'` skills, @-mention resolution at send time, editable "don't ask again" rules, and a durable approval queue UI.

## 6. Computer & environment control

**Built:**
- A bwrap sandbox with fail-closed `AllowDomains` (`EC/sandbox.rs:4`, `EC/executor.rs:156`).
- `code_interpreter` with a Space-scoped durable workspace: lease, hydrate, CAS promote; S3.3 complete 2026-09-12 (`EC/code_interpreter.rs`, `EC/workspace_promote.rs`).
- Background processes (S4.2).
- A hosted governed browser with live frames and human takeover (manual use live-proven; `SPA/features/dashboard/home/BrowserChrome.tsx`).
- Browser screenshot relay with a renderable allowlist (SVG/HTML excluded).

**Partial:** agent-driven browser runs. The gateway sends `grant_id: "session:{id}"` (`GW/domains/browser.rs:1235`), the broker mints random ids, and the run fails closed with 403 (CPA section 2.4). SRC.

**Decisions**
- **SandboxBackend trait (Hermes/DeepSeek/Claude convergence):** rejected for now. There is one backend. Revisit when a second exists (ledger:1358).
- **Sandbox full/partial vocabulary:** the probe exists, but no Go consumer reads it. Which service should read it is an open design call (ledger 2026-08-26). SRC search found no consumer.
- **User's own Chrome (extension/CDP):** parked by the owner ruling of 2026-09-28. "Chrome control means the hosted governed browser."
- **OS computer use (Endpoint Plane):** parked by the owner (CPA:56 "parked or ruled out"; the own-Chrome row is CPA:54).
- **Grok Bot shared-computer model, and UI-driving as the primary strategy:** NOT adopted. Typed actions come first and the browser is the fallback (COW section 4 #1, #3).
- **Planned:** teach-by-demonstration routines (COW #3, L), and a human-takeover flow for secrets, 2FA and CAPTCHA (COW #2).

## 7. Coding capability (Verevon is not an IDE)

The product decision is **"IDE-like capability, not IDE-like cold-start UI"** (CWP:193).
- **Built:** the LSP bridge client (`MG/lsp.rs:1`, `MP/bridges/lsp-bridge`), not deployed. H11, "post-write diagnostics instead of an LSP bridge", is deferred to Wave 6. Diff cards are built. The **Codex app-server is used as a provider route** (`IC/provider/codex_subscription.rs:26`). The 2026-09-23 decision keeps `gpt-5.6-terra` as the product route. The direct ChatGPT-OAuth Responses transport is built but opt-in, pending OpenAI's position on hosted multi-user use (CCA 8.1). SRC+DOC.
- **Deferred:** an editable, re-runnable code cell (H7, Wave 6).
- **License gate** (EIH section 1): codex is vendorable (Apache-2.0, but not its login/chatgpt crates). Hermes and pi are MIT. daytona is AGPL, ideas only. claude-code is leaked code: shapes only, zero code. cactus is source-available, so do not adopt it. DOC.

## 8. Memory, learning & personalization

**Built:**
- session-core `agent_memory` plus letta-bridge with **three live stores**: agentmemory, pgstore and memstore. CHD corrected an earlier "stub" claim.
- RRF fusion recall (`SC/memory_grpc.rs:207`; fixed 2026-09-17).
- SSE chat prefetch, 400 ms, with a Norwegian-aware trivial-prompt skip.
- A three-state provenance disclosure (`MG/memory_provenance.rs:103,168`; owner decision #3).
- `save_memory`/`recall_memory` in the agent loop (`EC/runtime_loop/agent.rs:4235`).
- Erasure propagation to the semantic tier (`SC/memory_erasure.rs`).
- Dreaming with supersession (`SC/dreaming.rs:197`) and a dedup tool.
- A Memory settings UI.

**Partial:**
- Extractor grounding exists behind `MEMORY_GROUNDING_ENABLED`, default off (`SC/memory_grounding.rs:90`).
- The G7 learning loop is wired: `SC/learning_events.rs:55`, `CC/internal/learning/learning.go:39` (MinConfidence 0.5), and the kill switch `LEARNING_REVIEW_ENABLED` (`CC/cmd/main.go:747`). **No skill has ever been proven from a real run** (I23 deferred). The review calls a paid model (claude-sonnet-4-5); the A0 fix is a global switch. A per-route rule is owner decision OD4. SRC+DOC.

**Decisions**
- **Memory-adapter registry (Hermes MemoryProvider/MemoryManager):** deferred (ledger:1390). Hermes members either already exist under other names or do not transfer: `backup_paths`, `flush_pending`, StreamingContextScrubber (CHD 7.9).
- **No pgvector column, no MemFS / git-backed rewrite, no second embedding pipeline** (LSR section 8).
- **Hermes / Honcho:** adopt only the "dialectic" standing user model, as a "mental model" profile (LEF #4, planned). Rejected: Honcho as a dependency, mem0's current ADD-only design, holographic memory, and RetainDB as a product (LEF section 1).
- **Learned-skill promotion gate:** a confirmed gap. Background-review candidates are matchable org-wide with no review. It is **deliberately not built** pending a UX/governance decision. The old `PromoteSkill` RPC was removed as "dead theater" (CHD 13.5-6).
- **Cross-tenant learned items:** never automatic. Only a curated vendor template library is allowed (CHD 13.4).
- **Semantic/exact answer cache:** REMOVED. Hosted LangCache was removed for residency and erasure reasons. The Dragonfly exact-match cache was removed 2026-09-16 because it "could not both hit and be correct" (`MG/langcache.rs:1`, a tombstone kept so the idea is not re-proposed). SRC.
- **Projects/folders with instructions:** deferred (I21) pending ADR-0003 precedence sign-off.
- **Planned:** skill-retirement loop (LSR #5), mental-model profile (LEF #4), Hermes-style FTS session recall (BKL #20).

## 9. Models & routing

**Built:**
- Intent + budget routing and a runtime-tunable routing policy.
- A fallback chain with ZDR, privacy-tier and residency gates (`IC/provider/{intent,routing_policy,fallback}.rs`).
- Providers: Anthropic direct and Azure Foundry; OpenAI and Azure OpenAI, including MaaS DeepSeek and Cohere; Codex subscription. `ModelFamily` has only two variants (`IC/provider/mod.rs:471`).
- A thinking dial (`MG/thinking.rs:18,21,29`).
- Modes budget/balance/genius and verbosity profiles (`SPA/shared/api/chat-client.ts:833`, `MG/verbosity.rs`).
- Structured outputs (`IC/provider/openai.rs:466`).
- Typed overflow classification that feeds fallback (`IC/provider/overflow.rs:100`).
- TOON compact encoding (`MP/rust/crates/mp-toon`, used in `MG/grounding.rs:684`).
- Anthropic `cache_control` and stable-prefix prompt order (`IC/provider/anthropic.rs:194`, `MG/prompt_context.rs:243`).

**Decisions / gaps**
- **Gemini:** absent. The seed row was soft-deleted *because* no adapter exists (`CC/migrations/0013_privacy_tier_columns.up.sql:23-36`). No plan to add it was found. SRC.
- **Local models (Ollama/vLLM):** absent (SRC search). The **Needle** self-hosted tool-calling model was cancelled after Phase 0: 2,048-token context and 47% accuracy (ledger:1990).
- **OpenAI `reasoning_effort` mapping:** not done, because there is no streamed reasoning to show (HAP 3.2). On the Codex route, `CODEX_APP_SERVER_THREAD_EFFORT` exists as a latency lever (CCA 8.1).
- **Provider failover on the subscription route:** none, by owner choice (CPA:131).
- **Prompt-cache hits:** never measured on any route. The OpenAI-route items are undeployed (CCA section 8).
- **pi silent-overflow heuristics:** not adopted, because there is no prompt-token accounting to base them on (HAP 4.2).
- **Model comparison side-by-side:** absent. I searched and found no plan.

## 10. Transparency, observability & control

**Built:**
- Context inspector (`MG/http_routes.rs:7932`, ChatPanels `ContextWindowPanel`). It is fetched only while open and has no Data Plane credential, by decision.
- The tabs chat/sources/artifacts/steps/trace (`SPA/features/chat/components/chat-types.ts:41`) with tool-call cards.
- Per-turn usage, cost and timing, with the cost-core ledger. The subscription route is counted as $0.
- Agent Run Console with proof bundles and a PlanPanel.
- The `memory_recall` and truncation (`stop_reason`) notices.
- Unknown-event logging (`SPA/shared/api/chat-client.ts:681`).
- The SSE-taxonomy freshness CI gate (`MP/scripts/gen-sse-event-taxonomy.py`, `.github/workflows/sse-taxonomy.yml`).
- The decision ledger and postmortems 0001 and 0002.

**Partial:** the reasoning trace is not visible on Terra; the recommended default is a collapsed summary (OD19). The inference-core metering decorator is built but not deployed (ledger:146). **Planned:** a live credit meter (warn at 80%, OD19), and the verb/object/outcome activity grammar (COW #1, AIF Phase 3). A DeepSeek-observability-level "see everything" UI is largely adopted (HAP WS-5, all 7 items).

## 11. Safety, permissions & governance

**Built:**
- Capability-before-dispatch with health attestation (`EC/capability_policy.rs:741`), plus a 9-entry `UNATTESTED_BASELINE` quarantine (`EC/runtime_loop/agent.rs:3954`).
- A risk classifier and the `requires_consent_to_disclose` and `is_restricted_context_write` predicates (`EC/permission/mod.rs:160,181,195`).
- The tenancy chokepoint `authorize_rpc` (`MG/grpc.rs:271`).
- Privacy tiers.
- Injection-defense reclassification of `web.search`/`web.read`.
- Moderation enforcement point (`MG/moderation.rs`). INF: effectiveness unknown.

**ZDR program (in flight, mostly dark and uncommitted):**
- No provider is attested, so every ZDR request fails closed (PSS section 5).
- The org ZDR switch is gated on an attested provider (Z-04; ledger:345).
- An **own-connection waiver** (signed, acknowledged, per-run, per-connection; ownership rule 17) is designed and built in parts: Z-11/12/13/17/18/21a. Z-15, enforcement in model-gateway, is **do not ship** (BP 2026-10-11).
- Contract goldens `MP/api/contracts/zdr-posture-matrix.json` (tracked) and `zdr-refusals.json` (untracked).

**Open:** editable "don't ask again" rules (CHD 13.3) and an external audit or certification (owner-only).

**Rejected anti-patterns:** an LLM on the permission hot path, reminder-injection sprawl, and dead-but-visible features (HAP WS-3); emoji or "reaction" approvals (COW section 4 #4); Hermes `network_mode: host` / fail-open defaults (CHD 4.5); and DeepSeek's web security posture, which is reachability-only, has a cross-session mux and serves its manifest unauthorized (CHD 13.2).

## 12. UI/UX & product design

**Decided:**
- Conversation is the command centre. Contextual surfaces open only on task events, and one surface is focused at a time (CWP sections 1 and 3, 12 invariants).
- Design-preview honesty badges stay until a backend exists. WorkflowBuilder is not built because "the model plane is an LLM agent loop, not an n8n DAG runner" (CHD 11.1; `SPA/features/agents/components/ChatbotStudio.tsx`).
- Space is a "calm control room" with honest placeholders (COW).
- Norwegian-first i18n.
- Presence stays in Spaces, not in /chat (OD19).
- Gamification limited to a per-org adoption map, with no per-user leaderboards. The reason is the Norwegian works-council context (CHD 13.4).

**Planned:** a keyboard shortcut sheet, block-level streaming re-render (CPA:45-46), and the activity grammar. A Trust Center exists at `V3/apps/verevon-web/src/app/trust`; its updated text is pending re-verification.

## 13. Platform architecture

**Built:**
- Split Rust/Go services.
- The BFF mints audience-bound delegated bearers per request. There are nine, including `x-sandbox-authorization`, chosen over a service token (ledger:3043).
- A transactional outbox to NATS with a live ACL proof for RUN_COMPLETED.

**Partial:**
- The audit-trail delivery fix is built but not deployed; 4,992 events were lost (`MP/docs/postmortem/0002-*`).
- Several in-memory, single-replica registries: cancel, queued input, plan grant and idempotency.
- Two `InvokeRequest` shapes (CHD 11.2; not verified as collapsed).
- Release artifact v3 has no signed candidate yet (ROADMAP).

---

## Planned / in-flight per docs (to separate from net-new)

1. **53-task program (owner rulings 2026-10-03):** contextual retrieval stays ON; Quarry option B rerank; per-org ZDR with per-connection consent; trust edits. 23 shipped-in-tree by 10-07. Passes 1-5 (10-10..12) built most of the rest dark. **Held:** Z-15 and QB-T09 (do not ship). Then QB-T17/T11/T14/T12 and Z-16. Nothing is deployed except the Control Plane wave. DOC `AUD/PROGRAM_SUMMARY_AND_STATUS_2026-10-07.md`, `AUD/BUILD_PASS_*`.
2. **Chat gap-closure plan (2026-10-01), waves 0-6:**
   - W0/W1 are done or deployed: A0-A5, A7, B8, B10, D2, D9, D12, E3, E7, F1-F4, G1, H1, H2, H5, I3, J1-J3, J9, J14.
   - Next: W1 Office/connector rows, W2 Terra proof window, W3 B-F end to end.
   - W4: Office/data outputs, connector reads, chat-core quick wins (shortcut sheet, charts, search, branching, rewind, OCR).
   - W5: writes and the deck path.
   - **W6 deferred:** E12 agent browser_agent, F12 Ask-mode connector reads, H7 editable code cell, H11 post-write diagnostics, I19 share link, I20 copy chat into a Space, I21 folders, I23 skill-from-real-run proof.
   - 20 owner decisions (OD1-OD20) carry recommended defaults. DOC `AUD/CHAT_GAP_CLOSURE_PLAN_2026-10-01.md:259` and section 7.
3. **AI-first creed:** M1 proves `tickets.create` end to end. M2 is a contract ratchet in CI plus ≥10 eligible actions. M3 is full classification. M4 is an outward MCP server, a WebMCP bridge, and SSR for shareable read-only resources. DOC `Verevon-ai-first-plan.md`.
4. **Learning:** grounding on by default, mental-model profile, skill retirement, ZDR-stamped RUN_COMPLETED (LSR section 8; LEF section 2).
5. **Space program:**
   - S3.3 workspace: done.
   - S4.2 processes: done.
   - S4.3 Watch: steps 1-3 done. The Model-runs adapter comes later.
   - Organisation/team Spaces, roster and shared-chat routing are required backend contracts (COW).
6. **Cowork research (COW):** activity grammar, human takeover for secrets, teach-by-demo routines, the Support Case Space "unit of work is the record" bar, a generalised draft-lease, and deciding the fate of the dormant Convex presence schema.
7. **Secure MVP release gates** (ROADMAP, still the gate list): signed artifact + rollback, NATS TLS/workload identity, capability health attestation, an eligible ZDR route, and a live approval continuation.

## Known gaps (code-confirmed or audit-confirmed; not just "not planned")

- **Reachability, not absence, is still the dominant gap.** Examples:
  - Plan approval is not consumed by any SPA turn.
  - Agent browser runs 403 on a stub grant id.
  - Scheduled runs are no-tool, 1,024-token calls.
  - Connector tools are listings only and Do-only. Do is off by default on the only route allowed for testing.
  - The continuation descriptor key is unset.
  - 9 capabilities are in an unattested quarantine, and the promotion ledger has 0 promoted rows.
  - The learning loop has never produced a skill from a real run.
  - Rerank and ZDR waiver are built dark.
- **No live proof** exists for: image, vision, voice, connectors against a real mailbox, Office outputs from the sandbox, citation chips, plan gate execution, and scheduled runs (CPA section 3).
- **Model breadth:** no Gemini, no local models, no OpenAI reasoning-effort mapping, and no side-by-side compare.
- **Chat table stakes:** FTS search, share link, rewind, durable branching, shortcut sheet, block re-render, editable REPL, live credit meter.
- **Agent/runtime:** no code mode, no todo recitation, no durable hook store, single-replica grant/queue/cancel stores, and the 300 s bearer ceiling on run length (OD8).
- **Governance:** no learned-skill promotion gate, no editable "don't ask again" rules, no tool-catalogue budget, and no external audit.

## Architecture constraints that limit what Verevon can adopt

From `apps/master-ownership-matrix.md` sections 0 and 3 (updated 2026-10-08), `CLAUDE.md`, and the ADRs:
1. **No cross-plane database access.** Model Plane and Quarry never touch Data Plane Postgres/Qdrant. Graphs, wiki, embeddings and chunks are Data-owned. *Effect:* memory, RAG and graph features (for example Graphiti, Letta MemFS or a pgvector memory column) must land in the owning plane's contract, or not at all.
2. **No independent embedding or reranking in Model Plane or Quarry** (rule 2/15). Rerank is a Data Plane endpoint. *Effect:* any "LLM rerank" or "local embedding" idea from a reference is non-compliant unless routed through the Data Plane.
3. **The Model proposes and Quarry executes or rejects browser actions** (rule 3). *Effect:* Manus- or Codex-style direct browser or computer control by the model loop is ruled out. Agent browsing needs a broker-issued grant, which is the missing piece today. The user's-own-Chrome and OS computer-use routes are parked.
4. **ZDR propagates through every content-persisting boundary** (rule 4). It is amended only by **rule 17**, a per-run signed waiver for a user's own connection, never org-wide. *Effect:* learning review, memory writes, caches, child-run answers, traces and resume buffers must all be ZDR-aware. The semantic answer cache was removed partly for this reason. With no attested provider, ZDR orgs get refusals, not degraded service.
5. **Identity and membership are Control-owned** (rule 9; bounded exception rule 18 for service principals on retrieval). Application projections only narrow access (rule 10), and **Space access never widens an owner-plane ACL** (ADR-0001). *Effect:* sharing, presence, projects and Space-scoped chat need Control decisions. A client invite list or a room-membership-is-enough model (Buzz canvases) is not adoptable.
6. **Provider execution never invents approval authority** (rule 12). An effectful call needs a tenant-bound bearer plus a signature over the exact effect. *Effect:* "auto-approve" tiers (Hermes auxiliary-LLM approval) and LLM-on-the-permission-path are excluded. The HITL continuation must carry the exact approved effect.
7. **Channel Plane is docs-only.** Slack- or Teams-style continuity and ambient agents need an ADR before code (COW section 4 #7).
8. **Capability health is earned per owner.** Source presence is not availability. The generic health route allowlists specific IDs only (AIF section 2). *Effect:* adopting a tool means owner contract + capability binding + attestation + forged-call denial test.
9. **Purpose-lock = offered tool list.** Hiding a tool forbids it. *Effect:* Needle- or Claude-style dynamic tool selection must not drop tools from the allowlist (ledger Needle entry).
10. **Two dispatch loops stay separate** (HARN-1/2). The read-only chat router must never gain side effects. Effects run through execution-core.
11. **License gate:** claude-code is shapes only; daytona (AGPL) is ideas only; cactus must not be adopted. codex can be vendored except its auth crates (EIH section 1).
12. **Owner test-route rule:** live proofs run only on the Terra subscription route. The cost of that is visible: features disabled on Terra cannot be proven, so their status stays "built" rather than "live".
13. **Tenancy chokepoint and delegated bearers** (nine kinds). A new upstream call needs its own audience-bound user bearer, not a service token, whenever the owner checks the subject (ledger:3043).

## Doc-vs-code discrepancies

| # | Doc claim | Code reality | Evidence |
|---|---|---|---|
| 1 | AIF (2026-08-25): `MODEL_EXECUTABLE_ACTION_IDS` "is an empty set" | 8 actions admitted, including `tickets.create` and `chat.submit_feedback` | SRC `SPA/shared/actions/model-eligibility.ts:126-135` |
| 2 | HAP 3.4: "`subagent::MAX_TOTAL_CHILDREN = 8`" | No such constant. `DEFAULT_MAX_CHILDREN = 8`, `MAX_CHILDREN_CEILING = 100`, and an org override via `resolve_child_ceiling` | SRC `EC/subagent/mod.rs:63,71,85` |
| 3 | CHD section 7.5 / HAP: provider-native context editing "needs a design call", "would only help one provider" | Native Anthropic `clear_tool_uses_20250919` + `compact_20260112` on by default since about 2026-09-17 (CCA section 7) | SRC `IC/provider/anthropic.rs:481,519` |
| 4 | `IC/provider/anthropic.rs` doc comment: model-gateway "skips both" local tiers for Anthropic-family models | `should_run_local_compaction` always returns `true`, so local and native compaction both run | SRC `MG/compaction.rs:145-149` vs `IC/provider/anthropic.rs:493-503` (code-comment drift) |
| 5 | BKL section 3 (2026-08-09): "Semantic response caching (CAG) … No competitor has" — listed as a moat | Removed. `langcache.rs` is a tombstone; the env vars are inert | SRC `MG/langcache.rs:1-60` |
| 6 | REFERENCE-MATRIX (2026-05-30): TOON, caveman terse modes, LLM Wiki, graphify, hermes memory, claude-code skills/plugins/voice all "❌ none" | TOON crate used in grounding; verbosity profiles; wiki/graph tools; skills, plugins and MCP registries; speech provider | SRC `MP/rust/crates/mp-toon/src/lib.rs:1`, `MG/grounding.rs:684`, `MG/verbosity.rs:1`, `MG/tool_loop.rs:3506` |
| 7 | MODEL_PLANE_STATUS/ROADMAP (evidence 2026-07-16): no continuation descriptor or dispatcher | AES-GCM continuation descriptor and approval-delivery worker exist. AIF says only the key is missing | SRC `SC/continuation_crypto.rs:1`, `EC/approval_delivery_worker.rs` |
| 8 | V2_PARITY: Gemini is merely "Missing"; earlier notes say there is a seed row | The Gemini seed row is soft-deleted in migration 0013 because no adapter can serve it | SRC `CC/migrations/0013_privacy_tier_columns.up.sql:23-36` |
| 9 | CHD 11.4: `ag_ui.rs` lacks org/user headers and delegated tokens; "fix or delete" | Route now behind `require_session`; the SPA reference client is explicitly kept unwired | SRC `GW/domains/ag_ui.rs` (imports `require_session`), `SPA/shared/api/ag-ui-client.ts:1-17` |
| 10 | CHD section 6 / an early matrix: "No durable/continuable subagent lifecycle" | Lineage, cold resume and consent-gated read are present | SRC `EC/runtime_loop/agent.rs:4262-4267` |
| 11 | CHD section 6: "capability-core tools/plugins/MCP registries remain stubs" | 1,802-line hardened MCP registry; single Capability registry with Kind | SRC `CC/internal/api/registry_apis.go:377` |
| 12 | COPILOT_APP_PARITY_2026-09-28:104 (as cited by CPA): agentic browser actions "at parity" | Agent browser runs fail closed on the stub grant id | SRC `GW/domains/browser.rs:1235` |
| 13 | CHAT_GAP_CLOSURE_PLAN 09-28 wave status: wide research "configurable to 100", built | Ceiling configurable in code, but the chat run builder resolves 4 rounds / 0 subagents by default and 10 / 2 on the subscription route | SRC `MG/sse.rs:755,820-870` |
| 14 | VEREVON_CHAT_DESIGN.md header: "Status: Proposed" (2026-08-17) | Large parts are implemented: chat-nodes registry, origin dimension, keyed channel, context inspector | SRC `SPA/shared/chat-nodes/*`, `GW/domains/chat/history.rs` |
| 15 | MODEL_PLANE_V2_PARITY (2026-07-11): hooks/MCP/plugins registry "in-memory" | MCP registry is Postgres-backed. Hooks are still in-memory (CPA), so this is half stale | SRC `CC/internal/api/registry_apis.go` |
| 16 | PAR section 1/2: `memory_recall` dropped by the SPA, no recall indicator, plans/todos uncalled | All wired (MemoryRecallNotice, PlanPanel, onUnknownEvent) | SRC `SPA/features/chat/components/ChatMessages.tsx` (MemoryRecallNotice), `SPA/features/agents/components/AgentRunConsole.tsx` (PlanPanel) |
| 17 | CAPABILITY_PROMOTION_LEDGER.tsv: every capability `source_only` | Code attests several capabilities live (memory pair, `cap.agent.spawn`, shell/sandbox, ticket.create, shipping). The ledger tracks release evidence, not health, but it reads as "nothing is live" | SRC `EC/runtime_loop/agent.rs:3940-3965` (attestation tables) |
| 18 | Ledger ordering note says "Newest first" | S3.3 / S4.x (September) entries are appended after August entries; the October ZDR and retrieval entries are at the top | DOC `MP/docs/decisions/ledger.md:21-25`, :3043-4613 |

*Confidence:* high for rows 1-11 and 13 (code read directly). Medium for 12 and 14-18, where the doc side was read second-hand through CPA or summary docs.
