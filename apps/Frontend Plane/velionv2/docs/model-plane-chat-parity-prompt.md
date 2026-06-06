# Model Plane — Chat Feature-Parity Requirements & Implementation Prompt

> **Purpose.** A single brief describing everything the **Model Plane** must
> support for the Velion chat to reach feature parity with **ChatGPT**,
> **Claude**, **Manus.ai**, and our **design references** (Dribbble chat
> inspiration + the velionv2 chat UI). Use it as (a) a planning/audit prompt for
> a backend agent, or (b) a product/eng checklist.

---

## How to use this as a prompt

> You are a senior backend/platform engineer working on the **Model Plane**
> (Go `model-gateway` + Rust/Python reasoning core, gRPC/proto contracts, NATS
> events, SSE streaming). Audit the current Model Plane against the capability
> matrix below. For every capability marked **Gap** or **Partial**, produce:
> 1. the **proto / gateway contract** changes (request fields + the SSE event
>    types and payloads the frontend can consume),
> 2. which **service / plane** owns it (Model Plane gateway, reasoning core, tool
>    runtime; **Data Plane v2** for RAG/retrieval; Quarry for live web scrape),
> 3. **effort + risk + dependencies**, and
> 4. a **phased rollout** (parity-essentials first, agentic last).
> Do not break the existing `profile:"chat"` plain-stream path. Prefer additive,
> versioned event types so the BFF can re-stream verbatim.

---

## Current baseline (what exists today)

**Flow.** Browser `streamChat` → BFF `POST /api/chat/stream` (auth, JWT mint,
optional Quarry scrape) → `POST {MODEL_PLANE_URL}/v1/invoke/stream` with body:

```jsonc
{ "content": "...", "model": "...", "session_key": "...", "thread_id": "...", "profile": "chat" }
```

→ SSE re-streamed **verbatim** to the browser.

**SSE events emitted today:** `connected`, `message` (`{ delta, request_id }`),
`done` (`{ model_used, input_tokens, output_tokens, request_id }`), `error`.

**Message data model (frontend):** `id, role, content, createdAt, tools[],
attachments[], status?(waiting|stopped|error), model?, modelUsed?, inputTokens?,
outputTokens?`. Reasoning schema (v1) also exposes `reasoning_trace[]`,
`confidence`, `reasoning_time_ms`, `alternative_explanations[]`.

**Frontend already renders:** streaming text, full Markdown **incl. GFM tables**,
code blocks, sender labels + relative time, a "thinking" indicator, an action
row (copy / 👍 / 👎 / regenerate / branch), an inline insight chip (model +
tokens), date dividers, and an "Agent activity" task timeline.

**Explicitly deferred in code:** server-side persistence, resume, deep-research,
agent tool-loop, multimodal.

---

## Plane ownership (who builds what)

This is **not** a Model-Plane-only effort. Ownership splits across planes — the
Model Plane orchestrates a turn, but RAG lives in the Data Plane.

- **Model Plane** — generation, token/`reasoning_delta` streaming, tool-call
  orchestration, model routing, `usage`. Per turn it *queries* Data Plane v2
  retrieval and turns returned passages into `citation` events.
- **Data Plane v2** — **owns RAG end-to-end**: document ingestion (user uploads
  *and* connector sources via Finspo-core / SharePoint Graph delta-sync),
  chunking + embeddings, vector + graph-index retrieval, and freshness via the
  doc-lifecycle NATS pipeline. Exposes an **org-scoped retrieval API** returning
  passages + source refs `{id,title,url,snippet}`. Reuses the existing
  graph-index that onboarding already seeds (see `docs/Onboarding-plan.md`).
- **Quarry** — live web scrape (already wired in the BFF) for browsing-style
  citations; complements, but is not, RAG.
- **BFF** — auth, JWT mint, attaches uploaded-file refs, SSE re-stream verbatim.

> **RAG freshness dependency:** Data Plane v2 has a verified gap — no
> `documents.updated` / `source_objects.changed` publisher, so retrieval can go
> stale on content change. RAG quality depends on closing that first.

---

## Capability matrix (parity targets → owning service + requirement)

| # | Capability | ChatGPT | Claude | Manus | Our UI wants it | Owning service + requirement | Status |
|---|------------|:------:|:-----:|:----:|:--------------:|--------------------------|:-----:|
| 1 | Token streaming | ✅ | ✅ | ✅ | ✅ have | `delta` events (exists) | ✅ |
| 2 | Markdown + tables + code + LaTeX | ✅ | ✅ | ✅ | ✅ (LaTeX TODO) | pass-through text; ensure no escaping; emit math as `$$` | 🟡 |
| 3 | **Extended thinking / reasoning trace** | ✅ | ✅ | ✅ | "Tenkte i Xs" + reasoning popover | separate `reasoning_delta` stream + `reasoning_time_ms`, `confidence` | 🟡 |
| 4 | **Stop / cancel** generation | ✅ | ✅ | ✅ | Stop button | cancellable stream + `stopped` terminal event | 🟡 |
| 5 | **Continue / regenerate** | ✅ | ✅ | — | regenerate exists (client) | server idempotent re-run by `request_id` | 🟡 |
| 6 | **Edit & resubmit / branch** | ✅ | ✅ | — | branch exists (client) | thread fork w/ parent message id | 🟡 |
| 7 | **Web browsing + citations** | ✅ | ✅ | ✅ | Sources tab | `citation` events `{title,url,snippet}`; Quarry wired | 🟡 |
| 8 | **File / document upload + RAG** | ✅ | ✅ | ✅ | attachments[] in UI · Sources tab | **Data Plane v2** owns ingest→index→retrieve (builds on graph-index + doc-lifecycle); Model Plane queries it per turn → `citation` events | 🔴 |
| 9 | **Vision (image input)** | ✅ | ✅ | ✅ | attachment images | multimodal `content` parts (text+image) in proto | 🔴 |
| 10 | **Image generation** | ✅ | — | ✅ | inline image render | `artifact{type:image}` event + asset URL | 🔴 |
| 11 | **Code execution / data analysis** | ✅ | ✅ | ✅ | Steps tab + outputs | sandboxed runtime + `tool_call`/`tool_result` events | 🔴 |
| 12 | **Tool use / function calling / MCP** | ✅ | ✅ | ✅ | tool chips, agent steps | tool schema in request; `tool_call`/`tool_result` events | 🔴 |
| 13 | **Artifacts / canvas** | ✅ | ✅ | ✅ | side-panel doc/code | `artifact` events `{id,kind,title,content,version}` | 🔴 |
| 14 | **Autonomous multi-step agent (Manus)** | 🟡 | 🟡 | ✅ | Agent activity timeline | plan/step protocol, long-running async tasks, progress, replay | 🔴 |
| 15 | **Memory / custom instructions / projects** | ✅ | ✅ | ✅ | per-user/project context | profile + memory store injected server-side | 🔴 |
| 16 | **Voice (STT/TTS / realtime)** | ✅ | 🟡 | — | voice mode toggle exists | realtime audio duplex endpoint | 🔴 |
| 17 | **Usage / latency / cost / confidence** | ✅ | ✅ | ✅ | insight chip + reasoning popover | `usage` event `{in,out,cost_usd,latency,confidence}` | 🟡 |
| 18 | **Model routing / capability registry** | ✅ | ✅ | ✅ | model picker | capabilities table → per-model feature flags | 🟡 |
| 19 | **Server-side persistence + sync + resume** | ✅ | ✅ | ✅ | cross-device history | thread store + `lastEventId` resume | 🔴 |
| 20 | **Safety / moderation / rate limit** | ✅ | ✅ | ✅ | graceful errors | moderation pass + structured `error` codes | 🟡 |

Legend: ✅ done · 🟡 partial / wiring needed · 🔴 gap.

---

## Proposed unified SSE event taxonomy (additive, versioned)

The BFF re-streams verbatim, so define ONE forward-compatible event set. Clients
ignore unknown types.

| event | payload | drives |
|-------|---------|--------|
| `connected` | `{ ok, request_id }` | stream-live indicator |
| `delta` | `{ delta, request_id }` | message text (exists) |
| `reasoning_delta` | `{ delta }` | collapsible "thinking…" trace |
| `step_update` | `{ id, title, detail, status }` | Agent-activity timeline / Steps tab |
| `tool_call` | `{ id, name, args }` | tool chips |
| `tool_result` | `{ id, status, output, error? }` | tool output / data-analysis |
| `citation` | `{ id, title, url, snippet }` | Sources tab |
| `artifact` | `{ id, kind, title, content, version }` | canvas / image / doc panel |
| `attachment` | `{ id, name, type, url, size }` | generated files/images |
| `usage` | `{ input_tokens, output_tokens, cost_usd, latency_ms, confidence }` | insight chip + reasoning popover |
| `error` | `{ code, message, request_id, retryable }` | error UI |
| `done` | `{ request_id, model_used, finish_reason, ...usage }` | finalize (exists) |

---

## Phasing / acceptance criteria

**Phase 1 — Conversation parity (ChatGPT/Claude basics).** Stop/cancel,
regenerate by `request_id`, `reasoning_delta` + `usage` events, citations from
the existing Quarry path, server-side thread persistence + resume via
`lastEventId`. *Accept:* user can stop mid-stream, regenerate, see thinking +
sources + token/latency, and reload a thread across devices.

**Phase 2 — Multimodal + tools.** Multimodal `content` parts (image input),
image-gen `artifact`, **Data Plane v2** RAG grounding (upload → ingest →
retrieve, surfaced as `citation` events), tool/function-calling + MCP with
`tool_call`/`tool_result`, artifacts/canvas.
*Accept:* attach an image/PDF and ask about it; model calls a tool and the chat
shows the call + result; a generated table/doc opens in a side panel.

**Phase 3 — Agentic (Manus parity).** Plan/decompose, long-running async tasks
with `step_update` progress, sandboxed code execution, live browser/computer
execution view, task replay, memory/projects, voice realtime.
*Accept:* a single prompt spawns a multi-step task that streams its plan, runs
tools/code, produces file artifacts, and can be replayed.

---

## Constraints & non-goals

- **Do not** break the current `profile:"chat"` plain stream. New events are
  additive; the BFF already re-streams unknown events.
- Keep formatting model-driven: tables/markdown render client-side
  (`ChatMarkdown`); the BFF nudges format via the `[Response formatting]`
  directive — no per-feature server formatting.
- Multi-tenant auth/audience minting already exists; reuse it for new endpoints.
- Confidence/latency/sector fields in the design's "Reasoning" popover must be
  **real** (`usage`/`reasoning` events) — no placeholder values shipped.
