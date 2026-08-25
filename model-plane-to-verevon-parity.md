# Model Plane → Verevon v3 Parity

**Date:** 2026-08-22 · **Scope:** what Model Plane's backend supports, what the
verevonv3 UI (chat, space, agents, inbox — everything else out of scope) is
missing, and what the gateway must do so the stream from backend to frontend is
seamless. Grounded in a same-day 13-agent read of all planning docs plus the
actual source of all five surfaces; every claim below is file-attributed.
Companion to `claude-hermes-deepseek.md` (the harness audit) and
`harness-adoption-execution-plan.md` (the execution plan).

---

## 0. The one architectural fact that shapes everything else

**The gateway needs almost nothing.** The BFF's chat/run SSE relay is a
byte-verbatim pipe, not an allowlist: `stream_chat`, `resume_stream`, and
`run_events_stream` transform only the *request* (persona/identity/ZDR
enrichment) and forward upstream SSE bytes untouched
(`apps/gateway/src/upstream.rs:1671` — `Some(Ok(bytes)) => yield bytes`), adding
only a 15s keep-alive comment and a synthetic terminal `error` on mid-stream
transport failure. A comment states the policy outright: *"raw byte-level SSE
proxy … no event-name allowlist"* (`domains/chat/streams.rs:164-171`).

**The silent drop point is the SPA.** `dispatchEvent` in
`src/shared/api/chat-client.ts:395` switches on `event.event` with **no default
arm** — any event outside its ~17 known cases is discarded without a trace
(comment at `:520`). Every new backend event lands in the browser and dies
there.

**The one gateway translator** is `ag_ui.rs` (`/api/v1/ag-ui/stream`), which
maps names to AG-UI frames — and its `_` fallback wraps unknown events as
`CUSTOM {name, value}` (`ag_ui.rs:650`), so even there nothing is dropped, only
renamed.

> **Rule for all future stream work:** new capability = (1) new `ChatEvent`
> variant in model-gateway's `sse_events.rs` (feature-family-gated), (2) zero
> gateway changes, (3) a new case in `chat-client.ts`'s `dispatchEvent` + a
> handler + a renderer. The gateway is not where parity work happens.

---

## 1. The stream contract, event by event

Backend = model-gateway `/v1/invoke/stream` (`sse_events.rs`'s `ChatEvent`) +
`/v1/runs/:id/events` (run events). Gateway = verbatim relay. SPA columns from
`chat-client.ts:404-517` and `run-console-client.ts`.

| Event | Backend | Gateway | SPA chat | SPA agent console | Gap |
|---|---|---|---|---|---|
| `connected` | ✅ (carries run_id/thread_id/model) | pipe | ✅ (`.ok` parsed, never consumed) | ✅ starts run-events stream | minor: consume `.ok` |
| `chunk` / `done` | ✅ | pipe | ✅ | ✅ | — |
| `stopped` | ✅ | pipe | ⚠️ **collapsed into `onDone`** (`chat-client.ts:426-428`) — a server-side stop renders as a normal completion | — | **fix: distinct terminal state** |
| `error {code, retryable}` | ✅ | pipe | ✅ | ⚠️ run-events `onError` swallowed (`AgentRunConsole.tsx:549-553`) | surface run-stream errors |
| `reasoning_delta` | ✅ | pipe | ✅ collapsed thinking trace | ✅ | — |
| `step_update` | ✅ | pipe | ✅ (doubles as HITL hook: raw `paused` triggers approval refresh) | ✅ | — |
| `tool_call` / `tool_result` | ✅ | pipe | ✅ tool pills | ✅ real names + summarized args | — |
| `citation` / `grounding` | ✅ | pipe | ✅ Sources tab | ✅ TrustPanel | ⚠️ `grounded` effectively always false unless SPA sends `rag`/`knowledge` features (AGENT_QUALITY_PLAN 0.2) |
| `artifact` / `attachment` | ✅ | pipe | ✅ Artifacts tab, versioned viewer | — | — |
| `usage` (tokens/cost/latency/confidence) | ✅ | pipe | ✅ insight chip | ✅ telemetry panel | — |
| `title` / `follow_ups` | ✅ | pipe | ✅ | — | — |
| **`memory_recall`** (NEW 2026-08-22, family `memory`) | ✅ count + latency_ms, emitted only when memory was genuinely injected | pipe — zero work | ❌ **no case → silently dropped** | ❌ | **add case + "recalled N memories" indicator; opt in via `features:["memory"]`** |
| **`stop_reason` on the final chunk** (NEW 2026-08-22) | ✅ captured per provider (incl. `stream_incomplete` for a broken connection); logged server-side | n/a — not yet on the SSE `SseChunk` payload (~18 shared construction sites, deferred) | ❌ | ❌ | backend exposes → SPA renders "answer may be truncated" |
| `approval_continuation_verified` | ✅ (run events) | pipe | — | ✅ verification timeline row | — |
| `browser_observation_received` | ✅ carries `screenshotRef`/`domSnapshotRef` (`run-console-client.ts:278-279`) | pipe | — | ⚠️ **parsed, only text rendered** — no screenshot view | render the ref (ZDR-honest states already exist in chat's run panel) |
| `browser_run_paused/resumed`, `browser_action_approval_required/decided` | ✅ | pipe | — | ❌ **parsed by client, wired to nothing** (4 events) | wire to approval deck |
| Resume replay | ⚠️ `stream_buffers` hold **text only**, in-process | forwards `Last-Event-ID` (`streams.rs:127-130`) | ⚠️ **client never sends a cursor**; replays from seq 0; tool/citation/usage/title events never replayed | run-events replay is durable + complete (the good path) | see §4.1 |

---

## 2. What the backend supports that the UI does not surface

Ranked by user value; all verified live in source this week.

1. **Long-term memory, end to end (all landed 2026-08-22).** SSE chat now
   prefetches memory per turn (timeout-bounded, trivial-prompt-skipped
   English+Norwegian, injected as the same `Relevant memory:` block the gRPC
   path uses) and emits `memory_recall`. The governed agent loop offers
   `save_memory` / `recall_memory` (ZDR-refused with an honest explanation;
   blocked in plan mode and delegated subagents; server-side scoping in
   session-core). Delegations auto-record a bounded memory
   (`record_delegation_memory`). **UI has none of it**: no recall indicator, no
   `memory` feature opt-in, no way to see/manage memories (the `/api/v1/memory`
   CRUD proxy exists in the gateway, unconsumed).
2. **`stop_reason` / silent-truncation detection.** The unary path always had
   it; streaming now captures it, including `stream_incomplete`. No surface
   renders "this answer was cut off" — the exact honest-UX moment the vision
   docs call proof-and-quality experience.
3. **Plans/todos/lineage.** `listPlans` / `listTodos` / `getSubagentLineage`
   are exported by clients and **called by nothing** (VEREVON_CHAT_DESIGN dead
   inventory). The console renders `plan_transitioned`/`todo_transitioned`
   events but has no plan *view*.
4. **Browser evidence.** Quarry produces snapshot-bound observations,
   screenshots, receipts (`verified|failed|unknown`), egress receipts. The
   console shows page title/URL text only; chat's `ChatLiveRunPanel` is ahead
   (honest ready/pending/withheld-ZDR/unavailable/failed screenshot states) —
   the console should reuse it.
5. **Proof Bundle in chat.** Rendered beautifully in the Agent Run Console
   (`:1455-1789`); chat turns that executed an approved effect have no receipt
   affordance — VEREVON_CHAT_DESIGN's "effectful turns are immutable +
   receipted" needs exactly this.
6. **Result handles / context inspector.** §23.6 result handles + `result_query`
   wired backend-side 2026-08-11; nothing in the UI lets a user see what the
   model's context actually contained (a top DeepSeek-observability item).

## 3. What the UI expects that the backend lacks

1. **Typed `context_pack` field on invoke** — inbox renders packs into prompt
   text because the contract has no field (`inbox-ai.ts:77-81`).
2. **Feedback contract mismatch — live 422.** UI sends
   `{requestId, rating:'positive'|'negative', note}`; gateway expects
   `{run_id, skill_id, rating:'good'|'acceptable'|'poor'}`
   (AGENT_QUALITY_PLAN 1.2). Skill quarantine/promotion is dead until aligned.
3. **Per-surface capability-state contract** (handoff 2026-07-13):
   `{state, reason_code, requires_approval, …}` should gate the composer's
   Browse/Plan/agentic toggles; UI still derives availability statically.
4. **Streaming STT + voice endpoints** — whole `voice-dictation` plan; today's
   dictation is browser `SpeechRecognition` only.
5. **Rich-event resume buffer** — see §4.1.
6. **Support-recurrence projection endpoint** (spec'd in verevon-inbox.md,
   unbuilt).

## 4. Gateway work list (short, because of §0)

> **Status 2026-08-22:** §4.1 and §4.2 are **DONE** — see
> `harness-adoption-execution-plan.md` item 5.7 for the full write-up. The buffer
> now stores SSE *frames* (not text), every emission carries a monotonic `id:`,
> resume replays each frame under its own event name, and the SPA sends
> `Last-Event-ID`. Legacy text-only records still deserialize so a deploy does
> not break in-flight resumes. §4.3's envelope-hygiene items remain open and are
> tracked in the dedup plan, not here.

1. **§4.1 Durable rich-event resume (the one real stream gap).**
   Model-gateway's `stream_buffers` are in-process and text-only; the SPA never
   sends `Last-Event-ID`; a reconnect loses tool/citation/usage/title events.
   The fix spans all three layers exactly once: model-gateway buffers *events*
   (Dragonfly, per CHAT_RESUME_AND_VERSIONS_SPEC step 6) keyed by `id:`; the
   gateway already forwards `Last-Event-ID`; the SPA sends its last seq. Until
   then, resume is honest-but-lossy.
2. **Nothing for new event types** — verify with a lint/test that the relay
   stays allowlist-free (a regression here would silently kill every future
   event; worth a contract test like `tool_retry_contract.rs`).
3. **Envelope hygiene items already tracked** (dedup plan OPEN list): BFF-6
   fabricated auditId/runId sites, BFF-2 Scrapfly/BrightData bypassing Quarry,
   BFF-5 ZDR gate NATS-path bypass, BFF-7 StudioStore in-memory (DSAR).
   These are correctness, not parity, but they gate trust claims the UI makes.

## 5. Per-surface gap lists

### 5.1 Chat (`src/features/chat`)
- Add `memory_recall` + `stop_reason` handling; request `features:["memory"]`.
- Split `stopped` from `done` (server stop currently renders as completion).
- Send `rag`/`knowledge` features so grounding actually engages.
- Resume: send `Last-Event-ID`; render "resumed, earlier details not replayed"
  honestly until §4.1 lands.
- Surface plan view (clients exist, uncalled) per VEREVON_CHAT_DESIGN Phase 3;
  the design doc's `ConversationNodeDefinition` registry is the vehicle.
- Proof-receipt affordance on effectful turns (reuse console's proof renderer).
- v2 shell geometry is already matched; no work.

### 5.2 Space (`src/features/spaces`)
- Backend-blocked, by design (honest "not published yet" states): Work +
  Knowledge tabs need owner-plane Space projections; Activity needs receipt
  correlation. **No UI work until those contracts exist** — the cockpit was
  built to receive them.
- Mention-turns should surface `memory_recall` like chat (same client).
- Delegation (UI-2c) stays unbuilt pending its safety design ("no safe partial
  slice" — approval_mode:auto is inert today and naive wiring would grant the
  entire capability surface).

### 5.3 Agents (`src/features/agents`)
- Wire the 4 parsed-but-unrendered browser events + screenshot refs (reuse
  chat's `BrowserChrome` view model — it's already imported).
- Surface run-events `onError` instead of swallowing.
- Fix feedback 422 (contract §3.2) — this unblocks the whole learning loop UI.
- Render `stop_reason`/`stream_incomplete` on the timeline.
- Blueprint workspaces/ChatbotStudio/WorkflowBuilder remain design previews by
  decision (DesignPreviewBadge honesty contract) — not parity debt.

### 5.4 Inbox/Support (`src/features/inbox`, `src/features/support`)
- Mount or delete `AiReviewQueue.tsx` (orphaned; only its test imports it).
- Typed `context_pack` once backend adds the field (§3.1).
- `sentiment` is permanently null — delete the dead badge or build the signal.
- Support rail's run-info display (tokens/cost/model/ZDR, "not reported" when
  absent) is the **house pattern** for observability UI — reuse it for chat's
  memory/stop_reason indicators rather than inventing a new one.

## 6. Doc-status corrections found while grounding this file

- `endpoint-map.md` (2026-06-10) says chat SSE emits 4 events — **stale**; the
  live contract is 17+.
- `HARNESS_PHASE1.md` "✅ feedback loop end-to-end" — **contradicted** by the
  later, code-verified AGENT_QUALITY_PLAN (4 breaks incl. the 422 and a
  registered-but-never-started Temporal workflow). Trust the quality plan.
- G7 learning loop "complete in code" — true AND inert: **nothing publishes
  RUN_COMPLETED to NATS in production** (quality plan). The execution plan owns
  this.
- Letta memory: `DeleteMemory` was a silent no-op and thread-delete does not
  erase memory (DSAR) — memory-lifecycle work must include erasure proof.
- `verevonv2-performance-*.md` snapshots are accessibility-tree dumps, not
  perf specs; they demand only shell geometry, already satisfied.
