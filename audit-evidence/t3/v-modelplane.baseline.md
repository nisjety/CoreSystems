# Verevon Model Plane baseline (code is ground truth)

Auditor role: Verevon baseline auditor for the Model Plane (the backend behind Verevon's chat and agent pages). This was a read-only audit: nothing was built, run, tested or changed.

Repo: `C:/dev/CoresSystem`, HEAD `afb83928` (2026-10-10 20:18 +0200). The newest Model Plane commit is `3c8bfb6c` (2026-10-08). There are 287 Model Plane commits since 2026-07-15. The working tree has **69 uncommitted Model Plane paths** (gateway `sse.rs`, `tool_loop.rs`, `auth.rs`, execution-core `agent.rs`, inference-core `grpc.rs`, session-core `dreaming.rs` and others), which are build passes 1 to 5 of the 2026-10 ZDR/rerank programme. I read the **working tree**. Where a claim depends on an uncommitted file, it is source evidence only.

Provenance tags:

- **SRC** means read directly in source.
- **DOC** means internal documentation.
- **INF** means my inference, with the basis stated.
- **REA** (artifact reverse-engineering) was not needed: everything here is source.

Status words:

- **shipped** means present in source and wired into a reachable path. It does **not** mean deployed or live-proven. Every status doc says the current source is not a production release (`apps/Model Plane/MODEL_PLANE_STATUS.md:1-30`; `apps/Frontend Plane/verevonv3/apps/verevon-web/plans/system-audits/PROGRAM_SUMMARY_AND_STATUS_2026-10-07.md` §4, §7).
- **partial** means it exists but is gated, limited or unreachable on a main path.
- **stub** means scaffolding that does nothing real.
- **planned** means it appears in docs only.
- **absent** means I searched for it and did not find it.

Path abbreviations (all under `apps/Model Plane/` unless noted):

| Abbrev | Path |
|---|---|
| `MG` | `rust/services/model-gateway/src` (143,974 lines in total, including tests) |
| `IC` | `rust/services/inference-core/src` (32,663) |
| `SC` | `rust/services/session-core/src` (47,868) |
| `EC` | `rust/services/execution-core/src` (65,388) |
| `CR` | `rust/crates` (`mp-contracts`, `mp-orchestration`, `mp-tokens`, `mp-toon`, …) |
| `CAP` | `go/services/capability-core` (37,100 lines including tests) |
| `ORC` | `go/services/orchestrator-core` (15,148) |
| `COST` | `go/services/cost-core` |
| `SBX` | `go/services/sandbox-manager` |
| `BB` | `go/services/browser-broker` |
| `LB` | `go/services/letta-bridge` |
| `BC` | `go/services/bridge-core` |
| `PROTO` | `proto/model_plane/v1` |
| `BFF` | `apps/Frontend Plane/verevonv3/apps/gateway/src/domains` (outside the Model Plane) |
| `SPA` | `apps/Frontend Plane/verevonv3/src/features` (outside the Model Plane) |

---

## 0. One-paragraph verdict

The Model Plane is a large, real, Rust-hot-path / Go-control agent backend. It has two deliberately separate loops:

- **The plain-chat loop** lives in `model-gateway` (`MG/sse.rs:875` `invoke_stream_sse`, a function of about 3,700 lines, plus `MG/tool_loop.rs`). It is read-only: it refuses side effects and offers no MCP. It streams typed SSE events and does a lot of grounding, verification and honesty work. This includes in-gateway deep research, web fetch-then-answer grounding, a relevance gate, citations anchored to claims, a graduated confidence score with a verification pass, artifacts/canvas, a code interpreter, vision, image generation, memory recall, skills, compaction, resume, cancel and mid-run input.
- **The governed "Do mode" loop** lives in `execution-core` (`EC/runtime_loop/agent.rs`, 13,521 lines). It is a ReAct loop with a capability-policy → hook → permission → HITL pipeline, parallel tool dispatch, depth-1 subagents and a wide-research fan-out. It has real effect tools: provider actions, shipping, social and tickets.

Around the two loops sit the other services:

- `inference-core` routes to Azure OpenAI, Anthropic (direct and Azure Foundry), Cohere/Azure and OpenAI-compatible endpoints. It also routes to the **user's own ChatGPT subscription** through Integration Core's Codex app-server. It has a three-mode cost-aware intent router and evidence-bound ZDR gating.
- `session-core` is the single writer for threads, runs, events, memory (including "Dreaming" extraction), approvals, plans and terminal receipts.
- Go services own capabilities/policy, cron/tasks/watches, Temporal workflows, cost, sandbox leases, browser grants and Letta memory.

The plane is **ahead of every reference app on governance, tenancy, ZDR, provenance and honesty contracts**. It is **behind on**:

- user-visible breadth: there is no Gemini or DeepSeek-native provider, no OS or desktop computer-use, no share links, no server-side chat search, and no rewind;
- **reachability** of several built features: MCP tools cannot be invoked by either loop, agent browser runs cannot obtain a grant, Do mode starts with no conversation history and no authored instructions, scheduled runs are a single no-tool call, and hooks are not applied in Do mode;
- **production readiness**: there is no signed release artifact, no attested ZDR provider, and much of the 2026-10 work is uncommitted and undeployed.

---

## 1. Chat experience

| Capability | Status | How it works | Evidence |
|---|---|---|---|
| Streaming chat endpoint | shipped | `POST /v1/invoke/stream` sends SSE with a monotonic `id:` per frame. If `InferStream` fails, it falls back to unary `Infer` and sends the result in chunks. It never emits a fake `done`. | SRC `MG/http_routes.rs:137`, `MG/sse.rs:875`, `MG/sse.rs:4549` (`chunk_for_stream`); DOC `docs/STUBS.md` §1 |
| Typed rich SSE taxonomy | shipped | 23 `ChatEvent` variants. Each rich event is gated on an opt-in `features[]` family (artifacts, citations, memory, reasoning, steps, tools, usage); control events always emit. A generated doc is CI-checked against the SPA. | SRC `MG/sse_events.rs:144`; DOC `docs/sse-event-taxonomy.md` |
| Unary invoke | shipped | `POST /v1/invoke` (gRPC `Invoke` as well). | SRC `MG/http_routes.rs:136,8898`; `PROTO/gateway.proto:19` |
| Stream resume after reconnect or reload | shipped | Deltas are buffered per verified (org, user, request) in Redis or an in-memory backend. `GET /v1/invoke/resume/:request_id` replays from `Last-Event-Id`. A disconnect detaches and does not cancel. Agentic runs resume via `/v1/runs/:id/events?after_event_id`. | SRC `MG/stream_buffer.rs:1-24,59`, `MG/sse.rs:7331,7533`; DOC `docs/CHAT_RESUME_AND_VERSIONS_SPEC.md` (live-verified 2026-08-01) |
| Cooperative stop/cancel | shipped | Per-request AtomicBool bound to tenant and user. Also bound to the run, so a durable run cancel stops the stream. Emits `stopped`. In-memory, single replica. | SRC `MG/cancel_registry.rs:1-13,39,76`; `MG/http_routes.rs:140,6970` |
| Mid-run user input (steer) | shipped | `POST /v1/invoke/:id/queue` is delivered at the next tool-round boundary. The thread is taken from the prepared run, not from the client. | SRC `MG/queued_input.rs:1-20,163,244`; `MG/http_routes.rs:142,7406` |
| Idempotent send/regenerate | shipped | Claim/Cached/InFlight registry keyed by scoped key. Persisted start keys are keyed-MAC opaque values. | SRC `MG/idempotency_registry.rs:1-20,161`; `MG/managed_start_key.rs:1-6` |
| Auto thread title | shipped | Generated after the first exchange (non-ZDR only) and emitted as a `title` event. | SRC `MG/sse.rs:8033,4307` |
| Follow-up suggestions | shipped | A post-answer inference sanitised to short lines, emitted as `follow_ups`. | SRC `MG/sse.rs:8120,4354` |
| Thread list / reload / messages | shipped | `GET /v1/threads` (limit, space_id, origin), `/v1/threads/:id/messages`, `/v1/threads/:id/events` (envelope metadata only). Session Core is the single writer. | SRC `MG/http_routes.rs:144,177,181,7753-7766,8568`; `SC/grpc.rs:5405,5289` |
| Rename, pin, persona toggle per thread | shipped | `POST /v1/threads/:id/presentation` writes the title override, `pinned` and `persona_enabled`. | SRC `MG/http_routes.rs:148-151,8026`; `SC/grpc.rs:5548`; `SC/../migrations/0021_thread_presentation.sql`, `0042_thread_persona_toggle.sql` |
| Archive / durable delete (single + bulk) | shipped | Archive is non-destructive. DELETE is a durable erase contract. There is a separate internal Space-deletion adapter. | SRC `MG/http_routes.rs:144-165,8136`; `SC/grpc.rs:5675,5840,5911,362` |
| Pin a message into context | shipped | `pinned_message_ids` are hoisted so compaction never sheds them. | SRC `MG/http_routes.rs:6751`, `MG/sse.rs:1576,5503-5526`, `MG/compaction.rs:491,516` |
| Regenerate / edit-and-resubmit signals | shipped | The `regenerated` and `edited_resubmit` flags feed the implicit-dissatisfaction detector. | SRC `MG/http_routes.rs:6735,6757`; `MG/dissatisfaction.rs:184` |
| Edit-version siblings (1/N switcher) | partial | **Client-only** and limited to the latest exchange. Nothing is persisted, so versions vanish on reload. There is no server branch model. | DOC `docs/CHAT_RESUME_AND_VERSIONS_SPEC.md` "#49 part 2"; SRC `SPA/chat/lib/chat-versions.ts` (exists) |
| Conversation branching / fork | absent (server) | No branch or parent-message fields in `sessions.proto`. Searched `branch\|parent_message\|supersed\|sibling` in `PROTO/sessions.proto`: no hits. | SRC `PROTO/sessions.proto` (grep) |
| Checkpoint / rewind of a conversation | absent | `SaveCheckpoint` and auto-checkpoint compaction exist for runs. No user-facing rewind or restore. Searched `rewind\|restore_checkpoint` across the Rust services: no hits. | SRC `SC/grpc.rs:4774`, `SC/compaction.rs:1-8,144` |
| Server-side conversation search (FTS) | absent | `ListThreadsQuery` has no query field. No FTS index in the migrations list. | SRC `MG/http_routes.rs:7753-7766`; DOC backlog #20 |
| Projects / folders for chats | partial (client) | "Projects" are a per-browser `localStorage` structure. There is no server model. Spaces are the server-side container (a separate audit). | SRC `SPA/chat/lib/chat-organization.ts:1-30` |
| Shareable read-only link | absent | Searched `share_link\|shareable\|public_link` in MG and SC: none. Listed as "not built" in the 2026-10-01 audit. | SRC grep; DOC `.../CHAT_PARITY_AUDIT_2026-10-01.md` §3.1 |
| Import / export of chats | partial (client) | DOCX, XLSX, PPTX and print export are built in the SPA. There is no server export or import endpoint. | SRC `SPA/chat/lib/chat-export/{docx,xlsx,pptx,print}.ts` |
| Markdown / code / math / chart rendering | shipped (SPA side; gateway guides) | The gateway injects a chart-fence notice only when a chart is asked for (`format_guidance`). Rendering happens in the SPA (`ChatChart.tsx`, `chat-code-highlight.ts`). | SRC `MG/format_guidance.rs:146,153`; `SPA/chat/components/ChatChart.tsx` |
| Artifacts / canvas with versions | shipped | `create_artifact`, `update_artifact` and `read_artifact` are inline tools. Versions are produced by re-emitting the same id. Code-interpreter files become artifacts. Persisted artifacts are rehydrated on reload. | SRC `MG/artifacts.rs:1-40,462,481`; `MG/tool_loop.rs:2618,2679,2795`; `MG/sse.rs:5304` |
| Context inspector ("what is in the window") | shipped | `GET /v1/threads/:id/context` exposes Session Core's itemised `GetContextAssembly`, with per-segment token estimates. | SRC `MG/http_routes.rs:153-159,7919`; `SC/grpc.rs:6158`; DOC ledger 2026-08-22 "Context inspector is a read-only view" |
| Temporary chat (ZDR / persistence-free turn) | shipped | The ZDR path writes no thread, run, event, cache, memory or idempotency record and makes one model call. Unsupported fields get a typed refusal. | SRC `MG/sse.rs:8411,8513`, `MG/zdr_features.rs:1-12`, `MG/zdr_posture.rs:146,155` |

## 2. Multimodal and creation

| Capability | Status | How it works | Evidence |
|---|---|---|---|
| Image input / vision in chat | shipped | An attachment routes the turn to inference-core `AnalyzeImage`, and the analysis is streamed as the answer. This is one-shot: the vision answer **replaces** the normal pipeline for that turn. | SRC `MG/vision.rs:1-6,52`, `MG/sse.rs:5778`; `IC/provider/vision.rs` (Azure OpenAI vision deployments, gpt-4o) |
| Image generation | shipped | `generate_image` sends the prompt to `GenerateImage` (gpt-image-1 / dall-e-3 on Azure or OpenAI) and emits an `artifact`. | SRC `MG/sse.rs:5958,6200`; `IC/provider/vision.rs`; `MG/http_routes.rs:417,3641` |
| Image editing / inpainting | absent | Searched for edit/mask in `IC/provider/vision.rs` and `/v1/ai/images*`: only generate, analyze and ocr. | SRC `MG/http_routes.rs:417-419` |
| OCR | shipped | `/v1/ai/images/ocr` → `ExtractImageText`. | SRC `MG/http_routes.rs:419`; `PROTO/inference.proto:57` |
| TTS / STT | shipped (API) | `/v1/ai/speech` and `/v1/ai/dictate` (8 MiB audio cap) → `SynthesizeSpeech`/`TranscribeSpeech` on Azure OpenAI tts/whisper. | SRC `MG/http_routes.rs:419-428,3951`; `IC/provider/speech.rs` |
| Realtime voice session | partial | `/v1/ai/realtime/session` mints a realtime session from the provider broker (gpt-realtime, Azure/OpenAI). There is no governed duplex barge-in path. ZDR voice is refused. | SRC `MG/http_routes.rs:448-450,4891`; `IC/provider/realtime.rs`; DOC Oct-1 audit "Full-duplex voice … partial" |
| Voice pipeline in bridge-core | stub | The STT → LLM → TTS interfaces exist, but `main.go` never instantiates them. | SRC `BC/internal/voice/pipeline.go:1-25`; `BC/cmd/main.go:55-64` (only the webhook adapter is wired) |
| Video generation | shipped (API) | `/v1/ai/video/generate`, `jobs/:id` and `generations/:id/content` call Azure OpenAI Sora jobs. | SRC `MG/http_routes.rs:451-457,4971`; `IC/provider/video.rs` |
| Document intelligence | shipped (API) | `/v1/ai/documents/{analyze,layout,forms,receipts,invoices}` call Azure Document Intelligence prebuilt models. | SRC `MG/http_routes.rs:432-437`; `IC/provider/doc_intel.rs` |
| Translation / language analytics | shipped (API) | Azure Translator plus an LLM fallback. Azure Language handles sentiment, NER, key phrases, PII, detection and summary. | SRC `MG/http_routes.rs:429-447`; `IC/provider/translation.rs`, `IC/provider/language.rs` |
| Code interpreter (data analysis, charts, files) | shipped | `code_interpreter` (python or sh) runs in a per-call hermetic workspace: bwrap, read-only rootfs, no network, timeout, scrubbed output, 8 MiB/16 MiB caps. Files come back as artifacts. In a Space-scoped run it gets a durable hydrated workspace. | SRC `EC/code_interpreter.rs:1-34,79-82`; `MG/tool_loop.rs:2554-2617`; `EC/sandbox_lease.rs:1-8` |
| Office document generation (docx/xlsx/pptx/pdf) | partial | Produced by the code interpreter (python libraries) or by client-side export. There is no dedicated server document generator. | SRC `MG/artifacts.rs:1-8`; `SPA/chat/lib/chat-export/*` |
| File upload into knowledge (RAG) | shipped | `POST /v1/chat/documents` → Data Plane v2 document ingest. The `doc_indexed` consumer signals when the document is durably retrievable. | SRC `MG/http_routes.rs:196-199,7655`; `MG/doc_indexed_consumer.rs:1-12` |
| Embeddings API | shipped | `/v1/ai/embeddings` → `CreateEmbedding`. An EU residency gate is on by default. | SRC `MG/http_routes.rs:415`; `IC/config.rs` (`MODEL_PLANE_ALLOW_NON_EU_EMBEDDING`) |

## 3. Research and web

| Capability | Status | How it works | Evidence |
|---|---|---|---|
| Web search tool | shipped | `web_search` → Quarry-v2 search. It is offered when the Search toggle is on **or** a time-sensitive heuristic forces it. Paid providers are gated by tier and retention. | SRC `MG/tool_loop.rs:2874`, `MG/tools.rs:1-10`; `MG/sse.rs:1960-1969` |
| Fetch-then-answer grounding | shipped | The top hits are fetched and real passages are selected. Answers are no longer written from 150-character snippets. | SRC `MG/grounding.rs:1-15,127,438` |
| Source relevance gate | shipped | Off-topic hits are discarded before they can become citations. | SRC `MG/relevance.rs:1-15,541` |
| Deep research (in-gateway) | shipped | Plan (up to 9 sub-queries, verevon-budget) → concurrent search → concurrent read (16 pages) → synthesis report with `[n]` citations and a mandatory "could not be verified" section → a `document` artifact. Wall clock 410 s. It deliberately does not use Temporal. | SRC `MG/deep_research.rs:1-48,91,131,233-264,3238,3316` |
| Wide research fan-out (Do mode) | partial | `subagent.research_fan_out` runs N items in parallel through `join_all`, with a structured result per item. It can be disabled by a kill switch. It is Do-mode only, and Do on the subscription route is off by default. Budget: `AGENTIC_MAX_ROUNDS` defaults to 4. | SRC `EC/subagent/research_fan_out.rs:1-10,36`, `EC/runtime_loop/agent.rs:2331,634-641`; `MG/run_budget.rs:17-60`; DOC Oct-1 audit §2.1 |
| Citations anchored to claims | shipped | After the stream, each `[n]` marker is located and given a `claim_id` plus byte offsets. Grounding and citations are persisted with the answer. | SRC `MG/citation_anchors.rs:1-12,106` |
| Org knowledge RAG (hybrid + graph + wiki + contradictions) | shipped | `knowledge_search`, `knowledge_graph_search`, `knowledge_wiki_search`, `knowledge_contradictions` and `knowledge_list_documents` map to Data Plane v2 typed endpoints. `suggested_next_tools` hints are honoured. There is a best-effort fallback retrieval even without opt-in. | SRC `MG/tool_loop.rs:3375,3506,3534,3564,3696`; `MG/retrieval.rs:1-30,80`; `MG/retrieval_tools.rs:1-10,134,201`; `MG/retrieval_metadata.rs:1-10` |
| Per-turn source scope (Auto / KnowledgeOnly / WebOnly) | shipped | Enforced by pruning tools and re-checking at dispatch, with a `source_scope_applied` receipt. It is rejected in combination with agentic mode. | SRC `MG/normalize.rs:58-62`; `MG/sse.rs:932-935` |
| Grounding honesty contract | shipped | "No grounding" is stated explicitly, and failed retrieval degrades with a `retrieval_notice`. | SRC `MG/retrieval.rs:242`; `MG/sse_events.rs:144` |
| Verevon in-app read connectors | shipped | Inbox, social, insights, Brreg and shipping quotes are reachable from plain chat, through the same upstreams the UI uses. | SRC `MG/verevon_actions.rs:1-12,320`; `MG/tool_loop.rs:3588-3696` |
| OAuth provider connectors (Microsoft, Google, Meta, LinkedIn, Slack, GitHub, Notion, Shopify, Stripe, Okta) | partial | Do mode only: `list_provider_actions` / `execute_provider_action` through integration-corev2 with a curated catalogue. `gmail.messages` returns ids only and has no query filter. | SRC `EC/integration_tools.rs:8-40,73-102`; DOC Oct-1 audit §2.5 |
| Browser-agent web research inside chat | partial | The inline `browser_agent` arm needs `BROWSER_AGENT_URL`, and `inline_tool_allowed` excludes it, so it is not offered in chat. | SRC `MG/tool_loop.rs:639-644,3099-3175` |

## 4. Agent runtime

| Capability | Status | How it works | Evidence |
|---|---|---|---|
| Plain-chat tool loop | shipped | Infer with tools → parallel dispatch (`join_all`) → results injected → repeat. 12 rounds by default (ceiling 32). The tool phase streams. A `finish_tool_phase` control tool ends it. | SRC `MG/tool_loop.rs:1-12,52-67,4668,7156` |
| Governed Do-mode agent loop | shipped (dark on the subscription route) | `RunAgent`: plan transition → ReAct rounds through `execute_step_with_subagent` → managed terminal receipt. Default 12 rounds (ceiling 32); the gateway sends 4 unless overridden. Temperature 0.7, 4,096 max tokens. On the subscription route it is behind `AGENTIC_SUBSCRIPTION_ENABLED` (off by default). | SRC `EC/runtime_loop/agent.rs:1-36,251,259,270-274,567,850-852`; `CR/mp-contracts/src/agentic_subscription.rs:41-47,120`; `MG/sse.rs:8978,9527` |
| Do-mode context seeding | partial (gap) | A run starts with a **fresh history**: the system preamble plus the goal (and skills). There are no prior thread turns and no persona, org or Space instructions. An approved plan is prepended. | SRC `EC/runtime_loop/agent.rs:1185-1200`; `MG/sse.rs:9135-9150` |
| Parallel tool calls in a round | shipped | `join_all` in both loops. | SRC `EC/runtime_loop/agent.rs:1874`; `MG/tool_loop.rs:7156` |
| Subagents (delegation) | shipped (bounded) | `subagent.task` re-enters the same loop with an isolated history. `MAX_DEPTH = 1`. Default 8 children (ceiling 100). Children are registered as child runs, conclusions persist on the child run, and the parent needs consent to read them back. | SRC `EC/subagent/mod.rs:1-8,20,29,63,71`; `EC/runtime_loop/agent.rs:2609,2989,3045`; `EC/runtime_loop/subagent_results.rs:1-10` |
| Plan mode / plan proposals | shipped | The composer's `plan_mode` marks the run (in memory and through durable `SetRunMode`). `plan.propose` emits a structured plan, and a `plan_proposed` event goes to the stream. `/v1/runs/:id/plan-approval` grants autonomy, and an approved plan is prepended to the next run. | SRC `MG/http_routes.rs:6796-6806,214,7138`; `MG/sse.rs:6286,9998,8744`; `EC/plan.rs:1-10,26` |
| Plans / todos store | shipped | `plans`, `plan_steps` (one linear ordinal), todos and approvals, with tenant compare-and-set. There is no DAG. | SRC `SC/orchestration_store.rs:1-8`; `PROTO/orchestration.proto:16-47`; DOC `docs/MODEL_PLANE_IMPROVEMENTS_2026.md` §1a item 3 |
| HITL approvals (pause) | shipped | Under the `ask` posture a risky tool, or a capability `Ask` decision, returns `awaiting_approval`. The loop mints a durable `CreateApproval` and returns early. | SRC `EC/runtime_loop/agent.rs:4476,4558`; `EC/runtime_loop/mod.rs:519-522`; `MG/approvals.rs:1-14` |
| Post-approval continuation | partial | A worker leases `approval_delivery_outbox` rows and re-executes **only** `execute_provider_action`, `book_shipment` and `tickets.create` from an encrypted exact-effect descriptor, with a postcondition check. The **LLM loop itself does not continue** after approval. The worker idles until credentials and orgs are configured. | SRC `EC/main.rs:28-31`, `EC/approval_delivery_worker.rs:1-60,1280-1330`; `SC/continuation_crypto.rs:1-8`; `SC/../migrations/0022_encrypted_approval_continuation_descriptors.sql` |
| Postcondition / verified-outcome checks | shipped | Deterministic read-back verifiers check provider writes, shipping and browser procedures. A Proof Bundle and verification metrics are exposed. | SRC `EC/postcondition.rs:1-10`, `EC/runtime_loop/post_mutation.rs:1-8`; `MG/http_routes.rs:990,1029`; `PROTO/proof_bundle.proto` |
| Tool-call retry | shipped (both loops) | Transient-failure retry, 3 attempts with backoff, using one failure vocabulary pinned by a contract test. | SRC `EC/runtime_loop/retry.rs:37,149`; `MG/tool_retry.rs:1-10` |
| Tool argument validation / repair / grounding | shipped | Arguments are validated against the schema before dispatch. MCP argument repair returns field-level errors. A grounding gate refuses fabricated user-only arguments. | SRC `CR/mp-contracts/src/tool_arguments.rs:1-5`; `MG/argument_repair.rs:71,249`; `EC/runtime_loop/agent.rs:1123` |
| Context compaction | shipped (both loops) | Gateway tier 1 clears stale tool results and tier 2 summarises the head into one message, persisted for later turns. The model can call `reattach_context` to recover dropped turns. Do mode has tier 1. Anthropic-native `clear_tool_uses` is optional. | SRC `MG/compaction.rs:1-20,129,674`, `MG/context_reattach.rs:1-12,87`, `MG/sse.rs:5560,5621`; `EC/compaction_budget.rs:1-8`; `IC/provider/anthropic.rs:473-613` |
| Prompt-too-long recovery | shipped | inference-core types overflow, and the gateway compacts and retries. | SRC `IC/provider/overflow.rs:1-10`; `MG/compaction.rs:950` |
| Round-budget / wall-clock guards | shipped | A server-resolved budget (never from the client), a concluding round reserved when the budget is spent, and `EXECUTION_RUN_WALL_CLOCK_SECS`. | SRC `MG/run_budget.rs:1-60`; `EC/run_budget.rs:15,41`; `EC/runtime_loop/agent.rs:484-538` |
| Managed-run terminalization | shipped (source) | Start/heartbeat/terminal receipts and a recovery worker. "finalization_pending" is used instead of fake success. | SRC `SC/terminalization.rs:1-8`; `SC/main.rs:190-195`; `PROTO/sessions.proto:159-193` |
| Graceful drain on redeploy | shipped | `/readyz` returns 503 and in-flight turns finish before exit. | SRC `MG/drain.rs:1-20,103` |
| Background processes (S4.2) | shipped (Space-scoped, gated) | `process_start/read/stdin/signal/list` under bwrap. The registry lives in sandbox-manager. The lease must carry `space:processes`. | SRC `EC/process_tools.rs:1-8`, `EC/process_host.rs:1-8`; `SBX/internal/process/store.go`; `MG/http_routes.rs:349-350` |
| Watches ("notify me when …") | partial | Run watchers (AUTO-2) notify when a run finishes. The general Watch primitive (S4.3) has a process-output adapter. Its sweeper refuses to observe without a fresh Control reauthorizer. | SRC `CAP/cmd/main.go:298-376`; `CAP/internal/watch/*`; `MG/http_routes.rs:392-400` |
| Scheduling / cron | partial | The cron sweeper fires schedules → tasks → task executor → Temporal `ScheduledRunSupervision` → `ExecuteScheduledStep`. That step is **one no-tool, 1,024-token call on verevon-balance**. No result delivery was found. | SRC `CAP/cmd/main.go:316-326,383-400`; `CAP/internal/taskexec/workflow_dispatcher.go:46-56`; `EC/grpc.rs:497-525`; DOC Oct-1 audit §2.3 |
| Durable Temporal workflows | partial | 8 allowlisted workflows (InteractiveRun, ScheduledRun, DeepTask, WideResearch, EvaluatorOptimizer, MemoryConsolidation, SkillPromotion, FeedbackPromotion). **Only capability-core's task executor starts them.** Chat and Do do not use Temporal. Autoresearch is deliberately excluded (fabricated cost). | SRC `ORC/internal/orchestration/workflowreg.go:98-160`; `ORC/cmd/main.go:146-175`; `MG/deep_research.rs:18-22` |
| Evaluator-optimizer loop | partial | A Temporal workflow with per-leg durable checkpoints. It is reachable only through StartWorkflow. | SRC `ORC/internal/evaloptimizer/loop.go`; `ORC/cmd/workflows/evaluator_optimizer.go` |
| Hooks (PreToolUse / PostToolUse) | partial | The engine is real (allow/deny/ask, glob match). Rules come from the gateway's **in-memory** HookRegistry (gRPC `RegisterHook`). They are passed only on browser runs. **The Do-mode loop passes an empty `hook_context`.** | SRC `EC/hook/mod.rs:1-18,113`; `EC/runtime_loop/mod.rs:557,797`; `EC/runtime_loop/agent.rs:1923-1927`; `MG/runtime_registries.rs:2032-2057`; `MG/browser_run.rs:418` |
| Evals | partial | eval-lab-py has 14 YAML cases (HITL, org isolation, ZDR, compaction). `mp-slo` is an SLO harness. These are offline only. | SRC `python/eval-lab-py/cases/*`; `CR/mp-slo/src/lib.rs:1-8` |
| Trajectory recording / export | stub-ish | In-memory ring buffer, Atropos export over gRPC, published on NATS. Not persisted. | SRC `MG/trajectory.rs:1-10,159,217` |
| Team / coordinator mode | stub-ish | `TeamCreate/Delete/List` against an in-memory `TeamWorkerStore`. | SRC `MG/coordinator.rs:1-20`; `PROTO/gateway.proto:83-91` |

## 5. Tools, plugins and extensibility

| Capability | Status | How it works | Evidence |
|---|---|---|---|
| Built-in chat tools | shipped | count_words, reattach_context, reattach_skill, web_search, get_weather, get_statistics, code_interpreter, create/read/update_artifact, inbox_search/get_conversation, result_query, fetch_url, knowledge_* (5), shipping_get_quotes, insights_overview, social_list_*, brreg_lookup, recall_memory. | SRC `MG/tool_loop.rs:4323-4470,2541-3740` |
| Do-mode tool catalogue | shipped | Capability-bound: code_interpreter, web_search/fetch, web.search/read, knowledge_search, save/recall_memory, subagent.task, research_fan_out, list/read_subagent_result, reattach_skill, yr_weather, traffic, news, company_lookup, shipping (track/quotes/carriers/book), social (list/publish), provider actions, browser.observe/act, plan.propose, process_* (Space), tickets.create (server-resolved). | SRC `EC/runtime_loop/agent.rs:4086,4116-4330`; `EC/capability_policy.rs:741-836` |
| Capability policy gate | shipped | Every dispatch first calls capability-core `EvaluatePolicy`. A trusted server-side map binds tool names to capability ids. An Allow needs verified signed (Ed25519 JWS) evidence. Ask leads to approval. Unknown tools fail closed. | SRC `EC/runtime_loop/mod.rs:501-552`; `EC/capability_policy.rs:350-390,741-836`; `CAP/internal/policy/engine.go` |
| Capability registry, ranking, rollout, scopes, audit | shipped | `/api/v1/capabilities` (list, ranked by success, p95, cost, approval and incidents; upsert; rollout; scopes grant/revoke/resolve; audit log). Health attestation (global/tenant) comes from execution-core's heartbeat. | SRC `CAP/internal/api/capabilities.go:52-76`; `CAP/internal/registry/capabilities_store.go:344-425,553`; `EC/health_attest.rs:1004`; `EC/main.rs:18-24` |
| MCP server registration / connect / OAuth 2.1 + DCR | shipped | `/v1/mcp/servers` (list/register/delete/share), `/connect` (auto-detects OAuth), OAuth start/callback (RFC 9728/8414/7591). Tokens are stored encrypted in capability-core. There is a DNS revalidator and an SSRF guard. Only Streamable HTTP is supported; stdio is quarantined. | SRC `MG/http_routes.rs:327-343,3185,3333`; `MG/mcp_oauth.rs:1-15,105,158`; `MG/mcp_http.rs:1-12`; `MG/runtime_registries.rs:55,870`; `CAP/internal/api/registry_apis.go:926,974`; `CAP/cmd/main.go:280-297` |
| MCP staged disclosure / result handles | shipped | Above 40 tools the catalogue collapses to `mcp_catalog` + `mcp_call`. Large results become queryable handles (`result_query`). | SRC `MG/runtime_registries.rs:559-616,830`; `MG/tool_result_handles.rs:1-12` |
| **MCP tool invocation by the model** | **partial / effectively blocked** | Plain chat does not advertise MCP and denies `mcp__*` and `mcp_call`. Do mode **offers** org MCP tools, but `trusted_capability_id` returns `None` for `mcp__*`, so the capability gate **denies every call** ("tool has no governed capability binding"). | SRC `MG/sse.rs:2014-2018`; `MG/tool_loop.rs:639-644,3723-3731`; `EC/runtime_loop/agent.rs:654-658`; `EC/capability_policy.rs:361-362,832-836,965`; DOC `MODEL_PLANE_STATUS.md` "Dynamic MCP dispatch has no authoritative binding and is denied" |
| Skills | shipped | Disk-loaded and learned (session-core `agent_skills`) skills are matched per turn. Up to 3 are injected under a character budget and can be read back with `reattach_skill`. CRUD lives in capability-core and session-core, with ownership and sharing. | SRC `MG/skills.rs:1-8`, `MG/sse.rs:5662-5700`; `EC/runtime_loop/agent.rs:1006,1022`; `EC/runtime_loop/skill_budget.rs:1-8`; `CAP/internal/api/registry_apis.go:65`; `SC/grpc.rs:4977` |
| Plugins | stub | Durable CRUD over `plugin_packages`. Per its own doc, a plugin is "inert until a host loads its manifest", and no host loads one. Searched MG and EC for a plugin manifest loader: none. | SRC `CAP/internal/api/plugins.go:15-22,41`; `MG/http_routes.rs:380-391` |
| Slash commands | partial | The capability-core command catalogue has `/models` (→ ListModels) and `/compact` (→ CompactNow). There is also a gateway gRPC `ListCommands/ExecuteCommand` (in memory). No evidence was found that the composer uses them. | SRC `CAP/internal/commands/handler.go:1-60`; `CAP/cmd/main.go:401-403`; `PROTO/gateway.proto:159-160` |
| Custom instructions hierarchy | shipped (chat only) | ADR-0003: platform → org → Space → agent → persona. The BFF resolves these server-side and injects them ahead of identity context. **They are not passed into Do runs.** | SRC `MG/http_routes.rs:6844,6850,6859`, `MG/sse.rs:6375,6440`; DOC Oct-1 audit gap "Persona, org and Space instructions applied in agentic runs: not built" |
| Client-supplied tools | shipped | `InvokeRequest.tools` with the `tools` feature, merged in Do mode as untrusted, with built-ins winning. | SRC `MG/http_routes.rs:6807-6811`; `EC/runtime_loop/agent.rs:659-684` |
| Public API / SDK | shipped (partial docs) | `api/openapi.yaml`, a generated SDK and `generate-sdk.sh`. gRPC `ModelGateway` with 60+ RPCs. | SRC `api/openapi.yaml`, `api/sdk/`; `PROTO/gateway.proto:17-178` |
| TOON compact encoding | shipped (narrow) | `/v1/toon/encode`; the `mp-toon` crate. | SRC `MG/http_routes.rs:235,9461`; `CR/mp-toon/src/lib.rs:1-5` |
| LSP bridge | partial | `LspQuery` passes through to an external Python lsp-bridge, a compose overlay only. It is not agent-visible. | SRC `MG/lsp.rs:1-15,157`; `bridges/lsp-bridge/server.py`; `deploy/docker-compose.bridges.yml:41-48` |
| Code mode (tools callable from the sandbox) | absent | `MODEL_PLANE_IMPROVEMENTS_2026` §23.4 says greenfield. There is no tool-RPC bridge into `code_interpreter` (network disabled). | DOC `docs/MODEL_PLANE_IMPROVEMENTS_2026.md:172-176`; SRC `EC/code_interpreter.rs:14-16` |

## 6. Computer and environment control

| Capability | Status | How it works | Evidence |
|---|---|---|---|
| OS sandbox for tool processes | shipped (Linux) | Bubblewrap argv plus Landlock/seccomp with policy (ReadOnly / WorkspaceWrite) and network policy. Probed at boot. | SRC `EC/sandbox.rs:1-8,256,402`; `EC/policy.rs:1-8`; `deploy/seccomp-bwrap.json` |
| `shell` tool | partial | Read-only host commands, high risk, human-gated. It is **not** in the Do-mode offered catalogue, only reachable through `ExecuteStep`. | SRC `EC/code_interpreter.rs:5-9`; `EC/capability_policy.rs:767`; `EC/runtime_loop/agent.rs:4116-4330` (absent from the list) |
| Sandbox leases / durable Space workspace | shipped (Space-scoped) | sandbox-manager keeps Postgres leases, a CAS-backed workspace manifest, snapshots, `PromoteWorkspace` overlay merge, and promotion into Data Plane documents. It is **lease and bookkeeping only**: execution happens in execution-core's own container. There is no VM or container provisioning (no docker/firecracker/e2b in the code). | SRC `SBX/internal/lease/lease.go:1-20`, `SBX/internal/workspace/store.go`; `EC/workspace_cas.rs:1-8`, `EC/workspace_hydrate.rs:1-8`, `EC/workspace_promote.rs:1-8`; DOC ledger 2026-08-22 "defer a SandboxBackend trait" |
| Browser control (agent) | partial | The execution-core LLM planner proposes `BrowserAction`s; Quarry executes them and returns observations. This holds the architecture rule "Model proposes, Quarry executes". There are risk classes, cookie-use HITL and pause/resume/stop. The `/v1/browser/runs` start needs a broker-validated grant, but the BFF sends `grant_id: "session:{id}"`, which the broker never minted, so **agent browser runs fail closed**. | SRC `EC/browser_agent.rs:1-8,621`, `EC/llm_planner.rs:1-8`, `EC/quarry_agent.rs:1-8`, `EC/browser_agent/browser_risk.rs:1-8`; `MG/browser_run.rs:138-200`; `BFF/browser.rs:1235`; `BB/internal/grant/grant.go:358` |
| Browser suggest-action (single step) | shipped | `POST /v1/browser/suggest-action` proposes one safe action from Quarry evidence. | SRC `MG/http_routes.rs:221-223,2013` |
| Human takeover of a browser | absent (Model Plane) | Live frames and takeover are a Quarry/BFF manual-browser feature (live-proven per the Oct-1 audit). There is no Model Plane path. | DOC Oct-1 audit §2.4 |
| Desktop / OS computer use | absent (parked) | Parked by the owner. Searched `computer_use`/desktop in MG and EC: none. | DOC Oct-1 audit "OS-level computer use: parked or ruled out" |
| User's own Chrome | absent (ruled out) | | DOC Oct-1 audit |
| Remote devices / channels | stub | bridge-core has session registry, webhook channel adapter and delivery worker. The Channel Plane is docs-only. | SRC `BC/internal/channel/adapter.go:1-20`, `BC/cmd/main.go:55-64` |
| Git / worktrees / deploy-preview | absent | Not a coding product. The ledger rejects worktree isolation for subagents in favour of a per-run child ceiling. | DOC `docs/decisions/ledger.md:1821` |

## 7. Coding capability (harness techniques only, since Verevon is not an IDE)

| Capability | Status | Evidence |
|---|---|---|
| Code execution (python/sh) with file in/out | shipped | SRC `EC/code_interpreter.rs:24-34` |
| Diff intent detection from output | shipped (narrow) | DOC ledger 2026-08-22 "The `diff` tool intent is detected from output" (`docs/decisions/ledger.md:1563`) |
| Source-bound drafting review (deterministic checks on numbers, dates, schedules, word counts, revision preservation, campaign-copy contracts) | shipped (differentiator) | SRC `MG/source_validation.rs:1-3,2113`, `MG/numeric_evidence.rs:1-2`, `MG/calendar_context.rs:1-2`, `MG/schedule_evidence.rs:1-3`, `MG/revision_preservation.rs:1-3`, `MG/result_validation.rs:1-6`, `MG/document_contract.rs:1-2`, `MG/author_guidance.rs:1-10` |
| Codebase indexing / tab completion / PR bots / IDE integration | absent (out of scope) | Searched: no repo-index or completion code. Only the LSP pass-through. |

## 8. Memory, learning and personalization

| Capability | Status | How it works | Evidence |
|---|---|---|---|
| Long-term memory store | shipped | Session Core `agent_memory` (scoped, `is_latest` lineage) dual-written to letta-bridge for semantic recall. User-facing `/v1/memories` supports list, create (correct) and delete. A delete erases both tiers. | SRC `SC/memory_grpc.rs:1,350,444,524,607`; `SC/memory_erasure.rs:1-8`; `MG/http_routes.rs:192-193,8721,8787` |
| Per-turn memory prefetch + `memory_recall` event | shipped | Skipped for trivial prompts (a Hermes port, with Norwegian words), bounded by a timeout. Provenance is projected so the user sees whether a memory was "told" or "inferred". | SRC `MG/memory_prefetch.rs:1-10,90`; `MG/sse.rs:5191,2227`; `MG/memory_provenance.rs:1-10,168` |
| Memory tools | shipped | `recall_memory` in chat; `save_memory` / `recall_memory` in Do mode. Writes are refused under ZDR and in plan mode or subagents. | SRC `MG/tool_loop.rs:3179`; `EC/capability_policy.rs:808-809` |
| "Dreaming" automatic memory extraction | shipped | A periodic loop (`DREAMING_INTERVAL_SECS`) runs a deterministic phrase matcher plus an LLM extractor. Write-time grounding dedups, and user edits and forgetting take precedence. There is a reviewed dedup CLI. | SRC `SC/dreaming.rs:1-8,145-160`; `SC/dream_extractor.rs:1-8`; `SC/memory_grounding.rs:1-8`; `SC/memory_control.rs:1`; `SC/memory_dedup.rs:1-8`; `SC/main.rs:156-164` |
| Memory consolidation workflow | partial | A Temporal `MemoryConsolidationWorkflow` with a model summary. It is reachable only through StartWorkflow. | SRC `ORC/cmd/activities/memoryconsolidation.go`; `ORC/internal/orchestration/workflowreg.go:137-143` |
| Skill learning loop | shipped (on by default) | session-core publishes `RUN_COMPLETED` → capability-core sessionreview runs an LLM review of the transcript → `UpsertAgentSkill` → the gateway injects learned skills. Kill switch `LEARNING_REVIEW_ENABLED`. | SRC `SC/learning_events.rs:1-12`; `CAP/internal/sessionreview/consumer.go:1-30`; `CAP/cmd/main.go:742-808` |
| Implicit dissatisfaction → quarantine | shipped | Signals from regenerate, edit-resubmit, correction and near-duplicate questions are published on `mp.v1.feedback.rated`. A Wilson-bounded quarantine of skills follows. | SRC `MG/dissatisfaction.rs:1-15,184`; `MG/implicit_feedback.rs:1-15`; `MG/sse.rs:10630`; `ORC/cmd/activities/quarantine.go` |
| Explicit feedback → skill credit | shipped | `/v1/feedback`. The skills injected into a turn are recorded server-side, keyed by request_id. | SRC `MG/http_routes.rs:203,6571`; `MG/chat_turn_registry.rs:1-15` |
| Confidence calibration data | shipped | Scored-turn inputs are joined with later ratings into training rows. | SRC `MG/calibration.rs:1-15,127,133` |
| Fine-tuning (Azure OpenAI) | shipped (API) | `/v1/finetune/jobs` (JSON and multipart), deploy to tier, poller worker, monthly cap. | SRC `MG/finetune_routes.rs:1-20`, `MG/finetune_azure.rs:1-15`, `MG/finetune_poller.rs:1-10`; `SC/finetune_grpc.rs:1-8` |
| Persona / custom instructions | shipped (chat) | See category 5. | |
| Knowledge base (org) | shipped (Data Plane owned) | See category 3. Wiki proposals and review are proxied. | SRC `MG/http_routes.rs:516-545` (wiki proposals at :542) |
| Session search over past chats | absent | See category 1. | |
| Learning Plane (`apps/learning-plane`) | not related | This is the Teachme school product (classes, courses, attempts). It does not take part in model learning. | SRC `apps/learning-plane/README.md:1-5`, `apps/learning-plane/docs/STATUS.md` |

## 9. Models and routing

| Capability | Status | How it works | Evidence |
|---|---|---|---|
| Multi-provider fallback chain | shipped | `INFERENCE_PROVIDER_ORDER` (default `anthropic,openai`). Registers azure-openai, anthropic (direct), azure-anthropic (Foundry Claude), cohere (Azure), openai (OpenAI-compatible base URL, also MaaS models such as DeepSeek/Cohere on Azure) and openai-codex-subscription. Bounded retries honour `retry-after`. Provider HTTP timeouts are bounded. | SRC `IC/config.rs:209-214,286-326`; `IC/provider/fallback.rs:1,505-700`; `IC/provider/openai.rs:33,282` |
| Gemini / Google provider | absent | Searched `gemini` in the Rust and Go services: only a test literal (`MG/compaction.rs:1078`) and the roadmap data. | SRC grep |
| DeepSeek native API | absent (only through OpenAI-compatible MaaS) | `deepseek-v3-2` appears only as a model id served by the openai-compatible provider. | SRC `IC/provider/fallback.rs:2024`, `IC/provider/intent.rs:919-927` |
| Local models (Ollama, vLLM) | absent (except a generic OpenAI-compatible base URL) | Searched `ollama\|vllm\|lmstudio`: none. `OPENAI_API_BASE` could point at one (INF). | SRC `IC/config.rs:287` |
| Verevon auto modes (Budget / Balance / Genius) | shipped | Heuristic task complexity × the org's cost-core budget posture → model table. Defaults run gpt-5-nano … claude-opus-4-8. The table can be edited at runtime through the Session Core `RoutingPolicy` and `/internal/v1/router-policy`. Tool-bearing turns get a complexity floor and a tool fallback ladder. | SRC `IC/provider/intent.rs:1-60`; `IC/provider/routing_policy.rs:127-222`; `IC/http_health.rs:1-6`; `SC/routing_policy_grpc.rs:1-8` |
| Model picker catalogue | shipped | `GET /v1/models` lists per-model feature families plus a `cheap` flag. | SRC `MG/http_routes.rs:195,7593`; `IC/grpc.rs:51-66,394` |
| User's own ChatGPT subscription (BYO) | shipped (chat) / dark (Do) | Integration Core runs the official Codex app-server. The Model Plane never sees the OAuth token, only an opaque connection id. Typed refusals cover usage limit, reauth, rate limit and context too long. | SRC `IC/provider/codex_subscription.rs:1-60,142-156`; `IC/provider/codex_subscription_tools.rs:1-2`; `MG/subscription_failure.rs:1-10`; `EC/agent_route.rs:1-7` |
| Reasoning / thinking controls | partial | Quick, Standard and Deep profiles are resolved server-side. Anthropic gets `thinking.budget_tokens`; Codex gets `reasoning_effort`. **The OpenAI and Azure chat path sends no reasoning effort.** `reasoning_delta` events are streamed where the provider emits them. | SRC `MG/thinking.rs:1-10,36`; `IC/provider/anthropic.rs:402-428,714-718`; `IC/provider/codex_subscription.rs:142,155`; `IC/provider/openai.rs:444,560` |
| Prompt caching | partial | Anthropic `cache_control` breakpoints are set on tools, system and history. An OpenAI prefix-stable order is optional and off unless an operator sets it. inference-core has an in-memory tenant-scoped response cache (non-stream, TTL 300 s, skipped for ZDR). | SRC `IC/provider/anthropic.rs:173-361`; `MG/prompt_context.rs:1-23,363`; `IC/cache.rs:1-6` |
| Semantic answer cache | absent (removed) | The gateway cache was removed. `langcache.rs` is kept as a record of why, and its env vars are inert. | SRC `MG/langcache.rs:1-20` |
| Structured outputs | shipped | `structured_output_schema` → OpenAI `response_format: json_schema` (named wrapper). On Anthropic it becomes an instruction or tool. The subscription route supports an object schema. | SRC `IC/provider/openai.rs:459-486`; `IC/provider/anthropic.rs:726-731`; `IC/provider/codex_subscription_tools.rs:15-33` |
| Token logprobs → certainty | shipped (OpenAI-family) | Logprobs are requested where supported and folded into the confidence score. | SRC `IC/provider/logprobs.rs:1-10`; `IC/provider/openai.rs:276-291,459-461` |
| Real BPE token counting | shipped | `mp-tokens` replaced the bytes/4 budget estimate. | SRC `CR/mp-tokens/src/lib.rs:1-5` |
| Privacy tier / residency / sovereignty routing | shipped | `min_privacy_tier` is applied before the network with a typed fallback. Residency is declared per provider; the Global deployment is deny-by-default. A `sovereign_required` posture is applied to Data Plane retrieval. | SRC `IC/config.rs:259-283`; `CR/mp-contracts/src/dataplane_posture.rs:1-5`; `MG/sse.rs:1015` |
| Comparison / side-by-side models | absent | Searched for compare/arena in MG: none. | SRC grep |
| Step-level routing (RoutingDecision) | planned | Routing today is request-level only. | DOC `docs/MODEL_PLANE_IMPROVEMENTS_2026.md:155-157,620-722` |

## 10. Transparency, observability and control

| Capability | Status | How it works | Evidence |
|---|---|---|---|
| Per-turn usage + cost | shipped | A `usage` event carries tokens, cost (from the cost-core price catalogue; null when unknown, never fabricated) and confidence. | SRC `MG/pricing.rs:1-12`; `MG/sse.rs:10154`; `MG/confidence.rs:1-12,299` |
| Per-turn phase timing | shipped | A `turn_timing` event covers admission, context, tool rounds, provider intervals and the durable write. inference-core supplies `ProviderTiming`. | SRC `MG/turn_timing.rs:1-12`, `MG/sse.rs:519-528`; `IC/provider/timing.rs:1-8` |
| Step trace on answers | shipped | `step_update`, `tool_call` and `tool_result` events (feature-gated). Agentic orchestration events are mapped to step updates. | SRC `MG/sse.rs:10211,10245`; `MG/sse_events.rs:144` |
| Reasoning view | partial | `reasoning_delta` events appear only when the provider emits reasoning (Anthropic thinking, Codex). | SRC `MG/sse.rs:3206` |
| Graduated confidence + verification pass | shipped | A deterministic score from observable evidence (citations, tools, hedging, truncation, logprobs). A low score triggers re-querying the org KB with the answer's claims (the web only if this turn already had web access), and emits `verification`. | SRC `MG/confidence.rs:1-12,306`; `MG/verification.rs:1-10,93,112`; `MG/sse.rs:3726` |
| Result receipts | shipped | `result_receipt` for version-bound local checks. | SRC `MG/sse.rs:4086`; `MG/sse_events.rs:145-146` |
| Run history, run detail, system runs | shipped | `/v1/runs`, `/v1/runs/:id`, `/v1/runs/system` (cron-fired runs). | SRC `MG/http_routes.rs:207-212,1194`; `SC/run_service_grpc.rs:1-12` |
| Proof Bundle / verification metrics | shipped | `/v1/orchestration/runs/:id/proof-bundle` and `/verification-metrics`, used by the Agent Run Console. | SRC `MG/http_routes.rs:286-292,990,1029`; `SPA/agents/components/AgentRunConsole.tsx:275` |
| Subagent lineage | shipped | `/v1/orchestration/threads/:id/lineage`. | SRC `MG/http_routes.rs:298-300,1105` |
| Metrics / telemetry | shipped | Prometheus: gateway requests, durations and active streams; `inference_requests_total` / `inference_tokens_total` through a metered decorator so no RPC goes unmetered. OTEL OTLP. | SRC `MG/gateway_metrics.rs:1-4`; `IC/metered.rs:1-8`, `IC/metrics.rs:1-8`; `CR/mp-telemetry/src/lib.rs:1-5` |
| Security audit trail of AI actions | partial | The Session Core transactional audit outbox goes to Audit Core. The 2026-10 fix (subjects, stream manifest, requeue) is built and **not deployed**: 4,992 events never delivered. | SRC `SC/audit_publisher.rs:1-8`; `SC/../migrations/0043_audit_outbox_requeue.sql`; DOC `docs/postmortem/0002-session-audit-trail-never-delivered.md`; PROGRAM_SUMMARY §5 |
| Cost dashboards / analytics | shipped (API) | cost-core: usage, run cost, aggregate, users, analytics, entries, Azure actual cost, pricing, budget check, billing outbox. | SRC `COST/internal/server/server.go:134-149`; `COST/internal/azurecost/sync.go`; `COST/internal/postgres/billing_delivery.go` |
| Live per-run credit meter | absent | DOC Oct-1 audit "Live credit meter: not built". | |
| Implementation-status endpoint | shipped | `/api/v1/model-plane/implementation-status` (roadmap data). | SRC `CAP/cmd/main.go:251` |

## 11. Safety, permissions and governance

| Capability | Status | How it works | Evidence |
|---|---|---|---|
| Fail-closed JWT auth on every service | shipped | RS256 through JWKS, alg pinned, kid rotation. gRPC interceptors verify before the listener binds. Each downstream gets a separate delegated, identity-bound bearer. | SRC `MG/auth.rs:1-20`, `MG/grpc_auth.rs:1-6`; `IC/auth.rs:1`; `SC/auth.rs:1-6`; `EC/auth.rs:1-6` |
| Approval posture (server-resolved) | shipped | The profile maps chat to `auto` and deployed_agent to `ask`. The client can only ratchet the posture stricter. Do runs default to `ask`. | SRC `MG/profile.rs:1-20,95,119`; `MG/sse.rs:8631` |
| Permission modes + risky-tool classifier | shipped (coarse) | Auto/Ask/Deny. Risk is decided by **substring matching** on the tool name (delete, write, send, post, exec, book_shipment, …). | SRC `EC/permission/mod.rs:6-20,229-260` |
| Autonomy ladder | shipped | ReadOnly < WorkspaceWrite < DangerFullAccess. A per-call check, with a stated justification required (deepseek-harness design). | SRC `CR/mp-contracts/src/autonomy.rs:1-60` |
| Prompt-injection defences | shipped | Tool results get trust-class provenance and a deterministic fail-closed injection scan of the full payload before truncation. A semantic ContentSafety shadow pass runs on external payloads. Security events carry hashes only. | SRC `MG/moderation.rs:1-20`; `MG/semantic_screening.rs:1-10`; `MG/security_events.rs:1-10`; `EC/provenance.rs:1-8` |
| PII redaction (opt-in) | shipped | `pii_redaction_required` is applied before a provider call. | SRC `MG/moderation.rs:123` |
| Secret scrubbing | shipped | Step and checkpoint payloads, plus sandbox output (Bearer, JWT, sk-*, AWS, PEM). | SRC `EC/scrub.rs:1-8`; `EC/executor.rs:1-8`; `SBX/internal/redact/redact.go` |
| SSRF guards | shipped | `remote_trigger`, MCP OAuth discovery, MCP DNS revalidation, CIDR tables. | SRC `MG/tools.rs:1-10`; `CAP/internal/api/mcp_dns_revalidator.go`; DOC commits a41ab41b, 4d748ce6 |
| Zero Data Retention (ZDR) | shipped (fail-closed); no attested provider | One retention resolver (`mp-contracts::retention`) with a golden matrix. A signed claim cannot be downgraded by a request. Inference refuses every unattested modality before provider I/O. ZDR attestation is evidence-bound with a review-by date. A per-connection waiver records a retention receipt on the run. **No provider is attested, so ZDR turns are refused** (DOC). | SRC `CR/mp-contracts/src/retention.rs:1-5`, `CR/mp-contracts/src/zdr_refusal.rs:1-5`; `IC/provider/zdr.rs:1-8`; `MG/zdr_posture.rs:1-12`; `SC/retention_receipt.rs:1-8`; DOC `docs/ZDR.md`, PROGRAM_SUMMARY §5 |
| Rate limiting / org quotas / budgets | shipped | Per-org token-bucket RPM. The org-core spend ceiling is read from Control (fails open on an org-core outage) and passed to cost-core `/budget/check`. The run budget is enforced. There is **no reservation/commit**. | SRC `MG/rate_limit.rs:1-3,112`; `MG/org_quota.rs:1-12`; `MG/budget.rs:1-4,48`; DOC IMPROVEMENTS §1a item 2 |
| GDPR erasure | shipped | Org and user erasure consumers in session-core and cost-core. Space deletion with semantic-memory receipts. | SRC `SC/gdpr.rs:1-8`, `SC/gdpr_nats.rs:1-8`, `SC/space_deletion_reconciler.rs:1-8`; `COST/internal/consumers/org_erasure_consumer.go` |
| Space authority (Control-signed decisions) | shipped | Threads carry space_id and an audience revision. Revoked membership denies ownership. Sandbox, process, cron and watch effects require Control decisions. | SRC `SC/space_membership_nats.rs:1-8`; `EC/capability_client.rs:1-8`; `EC/scheduled_step_decision.rs:1-8`; `CAP/internal/cron/control_authorizer.go` |
| Tenant delegation for workers | partial | Per-principal org allowlists. `allowAnyOrg` is rejected in production loaders. Verified delegated grants do not exist yet. | DOC `MODEL_PLANE_STATUS.md` "Workload credentials …" |
| NATS transport security | partial | Distinct named principals. No TLS, mTLS or workload identity. | DOC `MODEL_PLANE_STATUS.md` |
| Signed release artifact | partial | Artifact v3 tooling exists. No candidate or rollback artifact has been built. | DOC `README.md`; `MODEL_PLANE_STATUS.md` |
| Enterprise admin / SSO | out of plane | Control Plane owns it. | |

## 12. UI/UX surface the Model Plane powers (backend view)

| Capability | Status | Evidence |
|---|---|---|
| Opt-in feature families keep a plain `chat` profile minimal | shipped | SRC `MG/sse_events.rs:1-9` |
| Norwegian-first prompts and refusal copy (Bokmål report sections, trivial-prompt words) | shipped | SRC `MG/memory_prefetch.rs:1-10`; `MG/deep_research.rs:240-246` (`## Kunne ikke verifiseres`, `## Kilder`) |
| Typed, localisable refusal codes (ZDR, subscription, capability) | shipped | SRC `CR/mp-contracts/src/zdr_refusal.rs`; `MG/subscription_failure.rs`; `EC/runtime_loop/mod.rs:532-548` |
| Agent pages (Agent Run Console, Fleet console, Chatbot studio, Workflow builder) | partial | The Run Console reads real run, proof and plan APIs. `WorkflowBuilder` uses static node maps (INF, design preview). | SRC `SPA/agents/components/AgentRunConsole.tsx:229-275`; `SPA/agents/components/WorkflowBuilder.tsx:13-16` |
| Keyboard sheet, block-level streaming re-render | absent (SPA) | DOC Oct-1 audit §3.1 |

## 13. Platform architecture

| Aspect | Reality | Evidence |
|---|---|---|
| Services | 4 Rust services (gateway :8080/:9090, session :8081/:9091, inference :8082/:9092, execution :8083/:9093) and 8 Go services (orchestrator :8084, capability :8085/:9097, sandbox :8086/:9094, browser-broker :8087/:9095, letta-bridge :8088/:9096, cost :8089/:9098, bridge-core :8091, nats-provisioner). | DOC `README.md` "Service Topology"; SRC `go/services/*`, `rust/services/*` |
| Two loops, separate on purpose | The read-only `dispatch_tool` in the gateway and the gated `execute_step_inner` in execution-core. The refusal boundary is the authority boundary. Unification was rejected after measurement. | DOC `CLAUDE.md` "Code Style"; `docs/postmortem/0001-harn-1-2-tool-dispatch-unification.md`; SRC `MG/tool_loop.rs:2430`, `EC/runtime_loop/mod.rs:467` |
| Protocols | HTTP+SSE at the edge, gRPC between services (tonic and Go), NATS/JetStream `mp.v1.*`, Temporal for durable workflows. No ACP, AG-UI or A2A. | SRC `PROTO/*.proto`; `CR/mp-events/src/subjects.rs:1-5`; `CR/mp-events/src/temporal.rs:1-3` |
| Persistence | Postgres for Session Core (45 migrations), capability-core, cost-core and sandbox-manager. Redis/Dragonfly for the stream buffer. MinIO for artifacts. Letta / agent-memory for semantic memory. | SRC `SC/../migrations/0001-0045`; `MG/stream_buffer.rs:10-24` |
| Process-local state that limits multi-replica | Cancel registry, plan-mode store, team store, hook registry, approvals cache, trajectories and command registry are in memory. | SRC `MG/cancel_registry.rs:11-13`; `MG/coordinator.rs:1-20`; `MG/runtime_registries.rs:1-18` |
| Rust portability | The hot path is already Rust. Go is used for control. Python only for offline labs. | DOC `CLAUDE.md` |
| Deployment | `scripts/compose.sh` for dev. The release path is a signed artifact v3 runner. The production overlay removes host ports. | DOC `README.md` "Quickstart"; `deploy/docker-compose.production.yml` |

---

## Planned / in-flight per docs

1. **Release gates (MVP)**, from `MODEL_PLANE_ROADMAP.md` phases 0-8 and `MODEL_PLANE_STATUS.md` "MVP release blockers":
   - signed candidate and rollback artifacts;
   - managed secrets for NATS and the Auth registry;
   - verified tenant delegation;
   - managed-run terminalization proven on a release database;
   - a deployed global health reporter;
   - an attested ZDR provider or an org retention policy;
   - Session Core database tests;
   - NATS TLS.
2. **2026-10 ZDR/rerank programme** (PROGRAM_SUMMARY_AND_STATUS_2026-10-07 §4, BUILD_PASS_4_5): 23 tasks were reviewed and are undeployed. 26 tasks were blocked; Z-15 (waiver enforcement) and QB-T09 were held. Further items: forwarding the degrade notice into `retrieval_metadata.rs`, `tool_loop.rs` and `sse.rs` (Z-41 item A); an audit-trail replay; Quarry calling the Data Plane rerank.
3. **MODEL_PLANE_IMPROVEMENTS_2026** proposals:
   - GraphPlan/GraphNode DAG execution with recovery levels (§6, greenfield);
   - planner/executor/verifier separation (§7);
   - per-call signed execution attestation (§8.1);
   - step-level routing and a `RoutingDecision` record (§9);
   - cost reservation/commit (§18);
   - eval architecture (§19);
   - loop-family definitions and stall detection (§21);
   - a codec router (§22);
   - progressive disclosure owned by capability-core, plus the MCP 2026-07-28 RC client migration (§23.1, §23.11);
   - MCP code mode (§23.4);
   - tool-call graph compilation (§23.7);
   - capability bundles (§23.10);
   - Agent Plugins packaging (§23.12);
   - skills as versioned packages with scorecards and supply-chain checks (§24);
   - LLM Wiki gardening (§25).
4. **Chat parity backlog** (`docs/verevon_CHAT_PARITY_BACKLOG.md`), still open by my reading:
   - #6 slash commands in the composer;
   - #7 checkpoint/rewind;
   - #9 persistent per-run workspace (Space-only so far);
   - #11 code mode;
   - #13 projects (client-only);
   - #20 full-text search;
   - #21 canvas selection edits;
   - #22 share links;
   - #24 human browser takeover from chat;
   - #25 multi-provider auth-profile failover;
   - #26 advanced voice.
5. **Oct-1 parity audit gaps** (`.../CHAT_PARITY_AUDIT_2026-10-01.md` §3):
   - wide-research scale;
   - scheduling from chat with the full agent;
   - browser use from `/chat` (real grant issuance);
   - persona and org instructions in Do runs;
   - a live credit meter;
   - full-duplex voice;
   - screen share;
   - mail-to-task;
   - share link;
   - multi-user chat presence;
   - moving a chat into a Space;
   - a third-party data-protection audit.
6. **GOAL.md product shell** items: CLI/TUI, IDE bridge, remote sessions, voice and channels as product surfaces; multi-scope memory (run, thread, workspace, user, org, global).

## Known gaps (code-verified)

1. **MCP tools cannot be invoked by the model on any path.**
   - Plain chat: not advertised, and the call is denied. SRC `MG/sse.rs:2014-2018`, `MG/tool_loop.rs:3723-3731`.
   - Do mode: offered, but every call is denied by the capability gate. SRC `EC/capability_policy.rs:361-362,832-836`.
   - Registration, OAuth, discovery and staged disclosure all work. Only the final dispatch does not.
2. **Do mode is context-blind.** The run sees the preamble plus the goal (and skills). There is no thread history, no memory prefetch and no persona, org or Space instructions. SRC `EC/runtime_loop/agent.rs:1185-1200`; `MG/sse.rs:9135-9150`.
3. **Do mode on the ChatGPT-subscription route is off by default and read-only unless a second switch is on.** SRC `CR/mp-contracts/src/agentic_subscription.rs:41-47`. Per the Oct-1 audit, the composer disables Do on that route, and that route is the one live-proven for chat.
4. **Hooks are not applied in Do mode** (an empty `hook_context`). The hook registry is gateway-memory only. SRC `EC/runtime_loop/agent.rs:1927`; `MG/runtime_registries.rs:2032`.
5. **The agent-driven browser cannot start.** The BFF sends `session:{id}` as the grant id. SRC `BFF/browser.rs:1235`; `MG/browser_run.rs:143-170`.
6. **Scheduled runs are a single no-tool 1,024-token call** with no delivery. SRC `EC/grpc.rs:497-525`.
7. **The post-approval continuation re-executes the effect, but the LLM loop does not resume reasoning** after approval. Only three action kinds are resumable. SRC `EC/approval_delivery_worker.rs:14-33`.
8. **No server-side**: chat search, branching, rewind, share links, projects or export. **No** image editing.
9. **No** Gemini, native DeepSeek or explicit local-model provider. **No** reasoning-effort control on the OpenAI/Azure route.
10. **Sandboxing is only bwrap inside the execution-core container.** There is no VM or container per session, no persistent per-chat workspace outside Spaces, no desktop and no computer use.
11. **The plugins registry is inert.** The slash-command catalogue has two commands, and no composer integration was found.
12. **Much process-local state** prevents multi-replica correctness for cancel, plan mode, hooks and teams.
13. **Release posture**: no signed artifact, no attested ZDR provider, the audit trail is not delivered live, and 69 uncommitted Model Plane files. DOC.
14. **The risky-tool classifier is a substring list**, which can mis-class names such as "create_artifact" (INF: "create" is in the list; `create_artifact` is only in the gateway inline loop, which is read-only, so the impact is low there). SRC `EC/permission/mod.rs:229-260`.

## Architecture constraints that limit what Verevon can adopt

- **The plane-ownership rules** (`CLAUDE.md` at the repo root; `docs/capability-ownership-matrix.md`):
  - one canonical owner per capability, with no cross-plane database access;
  - the Data Plane owns retrieval and rerank;
  - Quarry owns browser execution and capture: the Model only *proposes* browser actions, Quarry executes or rejects them (`EC/quarry_agent.rs:1-8`);
  - Integration Core owns OAuth tokens: the Model Plane never sees provider or ChatGPT tokens (`IC/provider/codex_subscription.rs:1-6`; `EC/integration_tools.rs:10-11`);
  - Control owns identity, quotas and Space membership (`MG/org_quota.rs:1-10`).
  - A Claude-desktop-style local agent that holds credentials and acts directly on the machine therefore needs a brokered design.
- **The two loops must stay separate.** The gateway loop must refuse side effects, and every effect goes through execution-core's capability → hook → permission → HITL pipeline. DOC `CLAUDE.md`; postmortem 0001. Any "one loop" redesign was explicitly rejected.
- **Capability-before-dispatch, fail closed.** Every tool needs a trusted capability binding plus a fresh health attestation plus signed decision evidence. Dynamic tools (MCP, plugins, client tools in Do) need a registry-owned binding before they can run. SRC `EC/capability_policy.rs:741-836`.
- **ZDR must propagate and cannot be downgraded.** A ZDR turn takes a persistence-free path: no thread, run, memory, cache, tool loop or title. This rules out memory, artifacts, resume, Do mode and approvals for ZDR users until a provider is attested. Approval continuations are retention-bearing, so ZDR callers are rejected before descriptor creation. SRC `MG/zdr_features.rs:1-12`; DOC `MODEL_PLANE_STATUS.md` "Approval delivery …".
- **Space access never widens an owner-plane ACL.** Space effects (sandbox, process, cron, watch, ticket) each need a fresh Control-signed decision. A Space agent "is not a security boundary". SRC `MG/http_routes.rs:6825-6833`; `EC/scheduled_step_decision.rs:1-8`.
- **Rust owns the hot path and Go owns durable control.** The Temporal tier exists but chat and Do do not use it. Deep research is deliberately in-process. DOC `CLAUDE.md`; SRC `MG/deep_research.rs:18-22`.
- **Licence gating on external ideas:**
  - Apache code can be vendored;
  - MIT code can be ported (Hermes and deepseek-harness ports are already marked in the code);
  - AGPL needs a clean room;
  - leaked proprietary code may contribute shapes only.
  - DOC `docs/external-ideas-harvest.md`; SRC `MG/memory_prefetch.rs:3`, `MG/memory_provenance.rs:3`, `CR/mp-contracts/src/autonomy.rs:9`.
- **The Channel Plane is docs-only.** bridge-core is scaffolding, so messaging-channel ingress (Hermes/OpenClaw style) has no owner yet.
- **No new registry for a single implementation.** The ledger defers a SandboxBackend trait and a memory-adapter registry until a second backend exists (`docs/decisions/ledger.md:1358,1390`). Adopting E2B, Daytona or VM sandboxes means building that seam first.

## Doc-vs-code discrepancies

1. **`MODEL_PLANE_STATUS.md` and `MODEL_PLANE_ROADMAP.md` (2026-07-16)** say there is no approval-continuation dispatcher and no health reporter configured. In the code, `EC/main.rs:18-31` spawns both the `health_attest` heartbeat and the `approval_delivery_worker`. Both were committed in August (34f30ee4 and later). The status docs are stale.
2. **The `SC/approval_delivery.rs:1-6` module doc** says the module "deliberately has no execution dispatcher". The dispatcher now exists in execution-core, so this in-code comment is stale.
3. **`docs/verevon_CHAT_PARITY_BACKLOG.md` §3** lists "Semantic response caching (CAG, user-scoped, ZDR-gated)" as a strength. `MG/langcache.rs:1-20` says the answer cache was removed and its env vars are inert.
4. **`docs/STUBS.md`** (verified 2026-07-11) says "both MCP transports are implemented — stdio_*/http_*". In the code, stdio is quarantined (`MG/runtime_registries.rs:55,870`) and only Streamable HTTP works. STUBS also says the Letta memstore is the backend; letta-bridge now has `agentmemory` and `pgstore` too.
5. **`MG/mcp_http.rs:1-12`** says the MCP REST bridge sidecar "was never built". `bridges/mcp-bridge/server.js` (207 lines) and a compose overlay exist. It is unused by the current Streamable HTTP path.
6. **`docs/GOAL.md` "Current implementation truth"** (May 2026) is now false in many places:
   - it says capability-core RPCs return Unimplemented: they are real;
   - it says there is no multimodal breadth: images, speech, video, documents and realtime exist;
   - it says there are no tasks or cron: they exist;
   - it says there is no `/v1/orchestration`, `/v1/memory` or `/v1/tasks`: these namespaces exist;
   - it says letta-bridge is in-memory only.
7. **`MODEL_PLANE_IMPROVEMENTS_2026.md` §1a (re-verified 2026-08-09)** says progressive MCP disclosure is greenfield. Gateway-side staged disclosure (`mcp_catalog`/`mcp_call`, threshold 40) landed on 2026-08-10 (`MG/runtime_registries.rs:559-616`), although not in capability-core as the document proposes.
8. **The `README.md` "Implemented by service" list** is a 2026-04 historical snapshot that omits most of today's features. The README itself carries a 2026-07-20 correction banner.
9. **The Oct-1 chat parity audit §2.1** says the run builder sends `max_rounds: 4`, `max_subagents: 0` literals. These are now resolved by `MG/run_budget.rs` from env, with defaults still 4 and 0, so the behaviour is unchanged unless the env is set.
10. **`docs/CHAT_RESUME_AND_VERSIONS_SPEC.md`** names "#1 server-authoritative sessions" as the precondition for persisted versions. Sessions are server-authoritative now, but versions are still client-only.
11. **`README.md` "Service Topology"** says MCP proxy is owned by model-gateway and "execution-core only proxies through it". That is true in the code, but the execution-core proxy path is dead behind the capability gate (Known gap 1).
12. **`docs/ROADMAP.md` (2026-07-13)** is superseded per `CLAUDE.md`, but still carries phase claims. Trust `MODEL_PLANE_ROADMAP.md` plus the code.

## What I searched for and did not find (basis for the "absent" records)

These are the grep patterns, with where I ran them:

- `gemini` in rust/services and go/services;
- `ollama|vllm|llama.cpp|lmstudio`;
- `rewind|restore_checkpoint|RestoreCheckpoint` in the Rust services;
- `share_link|shareable|public_link` in MG and SC;
- `branch|parent_message|supersed|sibling` in `PROTO/sessions.proto`;
- the `ListThreadsQuery` fields (no search);
- `docker|firecracker|gvisor|kata|e2b|daytona|exec.Command` in sandbox-manager;
- `plugin_packages|manifest` in the gateway, and plugin loading in EC;
- `computer_use`/desktop in MG and EC;
- `reasoning_effort` in `IC/provider/openai.rs`;
- edit/mask image endpoints in `/v1/ai/images*`;
- `compare`/arena in MG.
