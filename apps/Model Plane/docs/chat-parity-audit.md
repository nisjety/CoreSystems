# Model Plane — Chat Feature-Parity Audit & Implementation Plan

Audited 2026-06-04 against the real Model Plane code (not the brief's assumptions).
Scope: reach ChatGPT/Claude/Manus parity for the Velion v2 chat without breaking the
existing `profile:"chat"` plain-stream path.

---

## 0. Executive finding — the reframe

**The matrix's 🔴s overstate the work.** Code-grounded, the Model Plane already owns the
*primitive* for nearly every capability; the dominant gap is a **chat-stream exposure
layer**, not building features. Three additive pieces cover ~80% of the matrix:

1. **A unified, opt-in SSE event taxonomy** emitted from `/v1/invoke/stream` (the brief's
   set). Build the emitter ONCE; every later capability just emits into it.
2. **A few additive `InvokeRequest` fields**: `content_parts[]` (multimodal), `tools[]`
   (function schema), `attachments[]`, `features[]` (opt-in to rich events).
3. **Gateway fan-in** wiring that bridges existing service RPCs/NATS events
   (inference vision/image/speech, execution-core executor, orchestration plans/steps,
   session-core memory/threads, Quarry scrape) into those SSE events.

This is the "small but powerful / no duplicate systems / right tool for the job" path:
**reuse the canonical owners, add one thin exposure seam.** Net: most rows move 🔴→🟡.

---

## 1. Current baseline (VERIFIED in code)

| Service | Already built (relevant to chat parity) |
|---|---|
| **model-gateway** | `/v1/invoke` + `/v1/invoke/stream` (sse.rs): emits `connected`/`chunk`(delta)/`done`/`error`; **resume buffer** (`stream_buffer` + `invoke_resume_sse` + `Last-Event-Id`); **Infer fallback** when token-streaming is down. MCP transport (`mcp_jsonrpc`, `runtime_registries`). Approvals write-through, plan-mode, Quarry proxy, voice passthrough, per-org `rate_limiter`. Holds clients for inference/session/orchestration/sandbox/capability/**retrieval/knowledge/document/graph** (Data Plane v2). |
| **InvokeRequest proto** | Already has: `session_key`, `thread_id`, `content`, `model`, `provider`, `max_tokens`, `temperature`, `stream`, **`metadata` (Struct)**, `structured_output_schema`, `zdr`, **`max_cost_usd`, `max_tokens_budget`**. *Missing:* multimodal parts, tools, attachments. |
| **inference-core** | RPCs that ALREADY exist: `InferStream`, **`AnalyzeImage`/`ExtractImageText` (vision)**, **`GenerateImage`**, **`SynthesizeSpeech`/`TranscribeSpeech`**, **`CreateRealtimeSession`**, embeddings, translation, doc-intel, video. Real OpenAI+Azure providers. |
| **execution-core** | **Real sandboxed executor** (`execute_sandboxed`, `shell` tool wired in `runtime_loop`, bwrap on Linux / passthrough else), permission + hook gates, secret-scrub, `browser_agent`. |
| **session-core** | Durable threads/messages/runs/checkpoints; **`CompactNow`**; **`GetContextAssembly`** (policy→workspace→agent→user→thread→episodic→skill→retrieval = **memory injection already ordered**); `agent_skills`; `SetRunMode`; `orchestration_store` (plans/approvals). |
| **capability-core** | Registry (capabilities/models/mcp_servers/skills/routing/safety), models registry w/ `streaming`+`modality` flags, `/commands`, audit-log, G7 learning loop. |
| **orchestrator/orchestration** | Plans, steps, approvals, run-modes; NATS `mp.v1.orchestration.*`; `StreamRunEvents` (run-events SSE already in the Go gateway). |
| **Quarry edge** | `/v1/scrape`, `/v1/agent/*` (live browser agent loop). |
| **Reasoning schema v1** | `reasoning_trace[]`, `confidence`, `reasoning_time_ms`, `alternative_explanations[]`. |
| **NATS** | `mp.v1.*` events, stream-open/closed subjects, **usage envelopes** (tokens+latency) already published per turn. |

---

## 2. Unified SSE taxonomy (adopt the brief's; additive + versioned + opt-in)

The BFF already re-streams unknown events verbatim. Add the events below. **Gate richer
events behind a request opt-in** so `profile:"chat"` keeps emitting only
`connected`/`chunk`/`done`/`error` — the plain path is untouched.

- New `InvokeRequest.features` (`repeated string`) OR `metadata.features` — client lists the
  event families it understands (`reasoning`, `tools`, `citations`, `artifacts`, `steps`,
  `usage`). Gateway only emits a family if requested. Forward-compatible: unknown families ignored.
- Add `schema_version` to `done`.

| event | payload | gateway emit point | owner of the data |
|---|---|---|---|
| `connected` | `{ ok, request_id }` | start of `invoke_stream_sse` (exists) | gateway |
| `chunk`/`delta` | `{ delta, request_id }` | InferStream loop (exists) | inference-core |
| `reasoning_delta` | `{ delta }` | new reasoning channel from InferStream | inference-core |
| `step_update` | `{ id, title, detail, status }` | bridge `mp.v1.orchestration.*` → SSE | orchestrator-core |
| `tool_call` | `{ id, name, args }` | tool-loop dispatch | execution-core / gateway |
| `tool_result` | `{ id, status, output, error? }` | executor result | execution-core |
| `citation` | `{ id, title, url, snippet }` | Quarry scrape / Data Plane retrieval | gateway / Data Plane |
| `artifact` | `{ id, kind, title, content, version }` | image-gen / doc / canvas output | gateway / inference |
| `attachment` | `{ id, name, type, url, size }` | generated assets | gateway |
| `usage` | `{ input_tokens, output_tokens, cost_usd, latency_ms, confidence }` | done-time, from usage envelope + cost-core + reasoning | gateway/cost-core |
| `error` | `{ code, message, request_id, retryable }` | error path (exists; add code+retryable) | gateway |
| `done` | `{ request_id, model_used, finish_reason, schema_version, ...usage }` | terminal (exists) | gateway |
| `stopped` | `{ request_id, reason }` | cancel path (NEW terminal) | gateway |

**The single biggest lever:** one `RichEventSink` in `sse.rs` that maps a typed internal
channel (`ReasoningDelta`/`StepUpdate`/`ToolCall`/`ToolResult`/`Citation`/`Artifact`/
`Attachment`/`Usage`/`Stopped`) → SSE frames, gated by `features[]`. Build it in Phase 1;
every later capability just pushes into it. No per-feature SSE plumbing duplicated.

---

## 3. Corrected capability matrix (real status → gap → owner → effort/risk → phase)

Effort S/M/L; Risk L/M/H. "Has" = primitive already exists; "Gap" = the plumbing to expose it.

| # | Capability | Brief | **Real** | Has (primitive) | Gap (plumbing) | Owner | Eff/Risk | Phase |
|---|---|---|---|---|---|---|---|---|
| 1 | Token streaming | ✅ | ✅ | delta exists | — | gateway | — | — |
| 2 | Markdown/tables/code/**LaTeX** | 🟡 | 🟡 | verbatim pass-through | client KaTeX render; ensure no `$$` escaping (gateway already passes delta raw) | client | S/L | 1 |
| 3 | Reasoning trace | 🟡 | 🟡 | reasoning schema v1 | `reasoning_delta` SSE + separate reasoning channel from provider (Anthropic thinking / OpenAI reasoning); `reasoning_time_ms`+`confidence` on `usage` | inference-core + gateway | M/M | 1 |
| 4 | Stop/cancel | 🟡 | 🟡 | client abort; stream ends on disconnect | server-side cancel (kill upstream) + `stopped` terminal event + partial-usage bill | gateway | S–M/L | 1 |
| 5 | Regenerate | 🟡 | 🟡 | `request_id` on request | idempotent re-run keyed by `request_id`; persist the turn | gateway + session-core | M/L | 1 |
| 6 | Branch/fork | 🟡 | 🟡 | runs have `parent_run_id` | message-level fork w/ `parent_message_id` | session-core | M/L | 1–2 |
| 7 | Web browsing + citations | 🟡 | 🟡 | Quarry `/v1/scrape` wired in BFF | emit `citation` events from the grounding step | gateway + Quarry | S–M/L | 1 |
| 8 | File upload + RAG | 🔴 | **🟡** | Data Plane v2 retrieval/knowledge/document clients in gateway | ingest endpoint (reuse Data Plane) + retrieval grounding in invoke path + `citation` source refs | Data Plane + gateway | L/M | 2 |
| 9 | Vision (image in) | 🔴 | **🟡** | inference `AnalyzeImage`/`ExtractImageText` | `InvokeRequest.content_parts[] {text|image}`; gateway routes image parts to vision provider | gateway + inference-core | M/M | 2 |
| 10 | Image generation | 🔴 | **🟡** | inference `GenerateImage` | `artifact{kind:image,url}` SSE + a gen tool/intent | gateway + inference-core | M/M | 2 |
| 11 | Code execution | 🔴 | **🟡** | execution-core real executor (`shell`) | add `python`/`code` tool to `runtime_loop` + `tool_call`/`tool_result` SSE | execution-core + gateway | M–L/M | 2–3 |
| 12 | Tool use / MCP | 🔴 | **🟡** | MCP transport + tool dispatch | `tools[]` schema in `InvokeRequest` + tool-loop + `tool_call`/`tool_result` SSE | gateway + execution-core + capability-core | L/M | 2 |
| 13 | Artifacts / canvas | 🔴 | **🟡** | tool/image/doc outputs | `artifact{id,kind,title,content,version}` SSE | gateway + reasoning | M/L | 2 |
| 14 | Agentic multi-step | 🔴 | **🟡** | orchestration plans/steps/approvals/run-modes + `StreamRunEvents` + Quarry agent loop | bridge `mp.v1.orchestration.*` → `step_update` SSE; async long-running task surface | orchestrator-core + gateway | L/M | 3 |
| 15 | Memory / instructions / projects | 🔴 | **🟡** | session-core `GetContextAssembly` already injects memory/episodic/skill; G7 learning | add custom-instructions + project scope into context assembly + profile | session-core | M/L | 1–3 |
| 16 | Voice realtime | 🔴 | **🟡** | inference `Synth`/`Transcribe`/`CreateRealtimeSession`; gateway voice passthrough | realtime duplex endpoint (WS/WebRTC) bridging `CreateRealtimeSession` | gateway + inference-core | L/H | 3 |
| 17 | Usage / latency / cost / confidence | 🟡 | 🟡 | `done` tokens; NATS usage envelopes; `max_cost_usd` | `usage` SSE w/ **real** `cost_usd` (cost-core) + `confidence` (reasoning) + `latency_ms` | gateway + cost-core + reasoning | S–M/L | 1 |
| 18 | Model routing / registry | 🟡 | 🟡 | capability-core models registry (streaming+modality flags) | expose per-model feature flags to the picker (BFF/`ListModels`) | capability-core | S/L | 1 |
| 19 | Persistence + sync + resume | 🔴 | **🟡** | gateway resume buffer + `Last-Event-Id` (in-mem); session-core durable threads/runs/checkpoints | persist chat turns to session-core; back resume with session-core (not just in-mem) | session-core + gateway | M–L/M | 1 |
| 20 | Safety / moderation / rate-limit | 🟡 | 🟡 | capability-core safety policies; gateway `rate_limiter`; structured errors | moderation pass (pre/post) + structured `error.code`+`retryable` | gateway + capability-core safety | M/M | 1 |

---

## 4. Proto / gateway contract changes (additive, versioned)

**`proto/model_plane/v1/gateway.proto` — `InvokeRequest` (add fields, keep numbering):**
- `repeated ContentPart content_parts = 16;` — `ContentPart { oneof { string text; ImageRef image; FileRef file; } }` (multimodal; `content` stays for plain text).
- `repeated ToolSpec tools = 17;` — `ToolSpec { string name; string description; google.protobuf.Struct json_schema; }`.
- `repeated AttachmentRef attachments = 18;` — `{ string id; string name; string mime; string url; int64 size; }`.
- `repeated string features = 19;` — opt-in event families (gates rich SSE).
- `string idempotency_key = 20;` — regenerate/idempotent re-run (defaults to `request_id`).
- `string parent_message_id = 21;` — branch/fork.

**SSE:** new `event:` names per §2; payloads as the brief's table + `error.code`/`retryable`,
`stopped`, `schema_version` on `done`. The BFF needs **no change** to forward them (already
re-streams unknown events); the velionv2 client adds handlers per family (it already ignores
unknown events).

**Cancel:** `POST /v1/invoke/{request_id}/cancel` (or a NATS `mp.v1.stream.cancel`) → gateway
aborts the upstream `InferStream`, emits `stopped`, bills partial usage.

---

## 5. Phased rollout + acceptance

**Phase 1 — Conversation parity (almost all plumbing of existing primitives).**
Build the `RichEventSink` + `features[]` opt-in. Ship: `stopped`/cancel, idempotent
regenerate, `reasoning_delta`+`usage` (real cost/confidence/latency), `citation` from the
existing Quarry path, durable persistence + session-core-backed resume, per-model flags,
moderation + structured errors.
*Accept:* stop mid-stream; regenerate; see thinking + sources + real token/latency/cost;
reload a thread across devices.

**Phase 2 — Multimodal + tools (expose inference/execution/Data-Plane primitives).**
`content_parts` (vision via `AnalyzeImage`), image `artifact` (`GenerateImage`), file
upload + RAG (Data Plane retrieval) with `citation`, `tools[]` + `tool_call`/`tool_result`
+ MCP, `artifact`/canvas.
*Accept:* attach an image/PDF and ask about it; model calls a tool and the chat shows
call+result; a generated table/doc opens in a side panel.

**Phase 3 — Agentic (Manus parity; bridge orchestration).**
`step_update` from `mp.v1.orchestration.*`, sandboxed code-exec tool, live browser/computer
view (Quarry agent), replay (`StreamRunEvents` + resume buffer), memory/projects, voice realtime.
*Accept:* one prompt spawns a multi-step task that streams its plan, runs tools/code,
produces file artifacts, and can be replayed.

---

## 6. Constraints honored
- `profile:"chat"` plain stream unchanged: rich events are **opt-in via `features[]`**; without it the gateway emits only today's 4 events.
- Formatting stays model-driven (client `ChatMarkdown`); gateway never per-feature-formats — it only passes deltas + structured side-events.
- Reuse existing multi-tenant auth/audience minting for any new endpoint (cancel, ingest, realtime).
- `confidence`/`latency`/`cost` in the Reasoning popover come from **real** `usage`/`reasoning`
  events (NATS usage envelopes + cost-core + reasoning schema) — **no placeholders**.

## 7. Net effort estimate
Phase 1 is dominated by the one-time `RichEventSink` + persistence wiring — **M**, low new-capability
risk (everything it surfaces already exists). Phases 2–3 are mostly *exposing* built primitives,
with genuine new build only in: file-ingest pipeline (P2), code-exec tool hardening on Linux (P3),
and voice realtime transport (P3, highest risk).

## 8. Implementation status (this branch)

**Phase 1 — landed & verified:**
- `RichEventSink` + `features[]` opt-in (`sse_events.rs`); plain `profile:"chat"` path untouched.
- `stopped`/cancel: `CancelRegistry` + `POST /v1/invoke/{id}/cancel` + BFF/client `cancelChat`.
- `usage` event with **real** tokens + latency (cost/confidence null until cost-core join).
- `reasoning_delta` + `citation` plumbed through gateway→BFF→client (event-name dispatch).
- **Structured errors** (`ChatEvent::Error { code, message, retryable }`) — gateway emits, client
  surfaces `code`/`retryable`. (commits `3c469d5`, `c9bac33`)
- **Idempotent `/v1/invoke`** via client `idempotency_key` (`IdempotencyRegistry`, in-memory,
  Pending/Done + TTL, Drop-guard release). (commit `8000fd7`)
- **Persistence + cross-device resume**: writes via session-core (`prepare_run`/append) + read path
  `GET /v1/threads/:id/messages` → `ListConversation`, BFF `/api/chat/history`, client
  `loadThreadHistory`, and workspace `hydrateSessionFromServer` on select. (commits `1828820`, `e535366`)

- **Stream-path regenerate-dedup**: idempotency guard extended to
  `/v1/invoke/stream` — concurrent dup rejected (`duplicate_in_flight`), completed key replays its
  answer, claim released on stream end via the guard's Drop. (commit `4a77c9d`)
- **Moderation at the prompt boundary** (`moderation.rs`): always-on injection-defense framing of
  untrusted RAG context (`scan_injection`) + opt-in (`moderation`/`pii`) PII redaction before the
  prompt reaches an external provider. capability-core owns the policy; the gateway enforces.
  (commit `3f10354`)
- **Per-model feature flags** end-to-end: `ProviderCapabilities::feature_flags()` →
  `ModelInfo.features` → gateway `GET /v1/models` → BFF `/api/chat/models` → client `loadModels()`.
  (commit `82ecd1c`)

**Phase 1 — COMPLETE.** Remaining safety nuance: `content_safety` (toxicity) classification needs a
classifier model behind an inference-core moderation route (owner-correct) — not faked with a
keyword list.

**Phase 2 — landed & verified:**
- **RAG grounding via Data Plane v2** (`retrieval.rs`): opt-in (`rag`/`knowledge`/`citations`) →
  `Retrieve` → numbered system-context block + `citation` events on stream & fallback paths.
  Reuses the canonical retrieval owner — no new RAG store. (commit `d087f48`)
- **Vision input** (`vision.rs`): an image attachment routes the turn through inference-core
  `AnalyzeImage` (vision owner) — `select_image()` (url / inline base64 / data: URLs), `vision_stream()`
  streams the description; `InvokeRequest.attachments` + BFF/client forwarding. (commit `11585f6`)
- **Image generation → artifact** (`image_gen_stream`): explicit `generate_image` routes to
  inference-core `GenerateImage` (owner) → `ChatEvent::Artifact{kind:image}` + chunk. (commit `d2c905f`)
- **File-upload ingest**: `POST /v1/documents` → Data Plane `CreateDocument` (ingest owner); BFF
  `/api/chat/documents` + client `uploadDocument()` — closes the RAG loop. (commit `4a7d8ce`)
- **Function-calling foundation**: `InferRequest.tools/tool_choice` + `InferResponse.tool_calls`;
  OpenAI (`tools`/`tool_choice` + `message.tool_calls`) and Anthropic (`tools` + `tool_use` blocks)
  translation/parsing, unit-tested both ways. The prerequisite that was previously missing. (commit `0d04a88`)
- **Gateway tool-execution loop** (`tool_loop.rs`): `run_tool_rounds()` — infer-with-tools → emit
  `tool_call` → dispatch via a name→handler map (`web_search`→Quarry; unknown→error outcome) → emit
  `tool_result` → inject results as context → repeat (cap 3) → stream the final answer tools-withheld.
  Events on stream + fallback paths; BFF forwards `toolDefs`→`tools`; client dispatches both events.
  Loop/dispatch unit-tested; live tool execution + model tool-choice verify on the stack. (commit `062564f4`)

- **MCP tools in the loop** (`tool_loop::dispatch_tool`): `mcp__<server>__<tool>` calls route through
  the existing MCP registry (`handle_proxy_mcp_tool`, http + stdio, allowlist-enforced). Unit-tested
  name parsing. Reuses the registered-server registry — no new transport. (commit `86114b85`)
- **`fetch_url` tool**: reads a specific page via Quarry scrape (complements `web_search`); content
  truncated. (commit `83c346a1`)

- **artifact/canvas** — full surface: `artifact` event → client chunk → `ChatMessage.artifacts` →
  `ArtifactsPanel` tab with prose (ChatMarkdown), code (`<pre>`), and **image** (`<img>` via
  `imageArtifactSrc`) rendering. (commit on the image-render gap; rest pre-existing)

**Phase 2 — COMPLETE.** (The `code-exec` tool is tracked under Phase 3 below, gated on sandbox safety.)

**Phase 3 — landed:**
- **Replay / resume**: `stream_buffer::replay_after` (memory + Redis, `Last-Event-Id` cursor) +
  `GET /v1/invoke/resume/:request_id`; tested. Reconnect replays deltas after the cursor + terminal done.
- **Agentic `step_update`**: `run_events_sse` bridges all `mp.v1.orchestration.*` events
  (`orchestration_event_to_step_update`) into the unified `step_update` taxonomy, so a run's plan/
  todo/subagent/approval progress renders in the chat Steps timeline. 2 unit tests. (commit `8e503736`)

- **Voice realtime — server side built**: gateway already exposes `/v1/ai/realtime` (mints
  `CreateRealtimeSession` — the server's role in the WebRTC pattern), `/v1/ai/speech` (TTS via
  `SynthesizeSpeech`), and STT (`TranscribeSpeech`). The browser connects the minted session directly
  to the provider for live audio.

**Phase 3 — remaining (each gated on safety, live A/V, or a large integration — NOT headless-buildable+verifiable):**
- **Sandboxed code-exec** — architecturally belongs *inside* an orchestration run: `ExecuteStep`
  requires `run_id`/`step_id` and enforces `permission_mode` + hooks, with the sandbox below it. So
  it needs the chat→orchestration agentic-run integration AND Tier 2 sandbox isolation (in-progress
  separate workstream). A standalone gateway `dispatch_tool` arm would be the wrong shape (no run
  context) and would run unsandboxed. The `step_update` *output* of such a run is already wired.
- **Voice realtime — frontend**: a browser WebRTC mic/audio client against the minted session; only
  verifiable with live audio I/O.
- **Live browser/computer view**: streaming an interactive Quarry agent session to the UI; needs a
  live Quarry agent + a frontend viewer. (`web_search` + `fetch_url` cover non-interactive web.)
- **memory/projects** — product surface.

These four require either the in-progress sandbox workstream, a live A/V/browser environment, or a
large multi-session agentic-run integration — none completable+verifiable in a headless coding session.

All landed work reuses canonical owners (Data Plane v2 retrieval/knowledge/graph/wiki, Quarry v2
web, inference-core providers, session-core conversations, capability-core safety) — no duplicate
subsystems introduced.
