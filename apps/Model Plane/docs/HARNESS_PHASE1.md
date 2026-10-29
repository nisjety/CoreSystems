# Harness Engineering — Phase 1

> **Status**: in progress (2026-05-27). This doc is the authoritative spec for the
> harness-reliability work. When code disagrees with this file, fix the code or
> update this file in the same change.

Harness engineering = the execution system *around* the model: orchestration,
state, retries, guardrails, observability, HITL. Prompt = words, Context =
knowledge, Harness = execution. This phase closes the gaps between "the model
answers well" and "the agent runs reliably in production".

## 0. Key finding — most of this is already designed

The Phase-1 harness contract already exists at the proto level:

- `proto/model_plane/v1/orchestration.proto` — `OrchestrationCoreService`:
  Plan / Todo / Approval CRUD + state-machine transitions + `StreamRunEvents`
  (server-streaming task-graph feed, SSE-compatible at the gateway).
- `proto/model_plane/v1/events.proto` — event taxonomy incl. `PLAN_CREATED`,
  `APPROVAL_REQUESTED`, `RUN_PAUSED_FOR_APPROVAL`, `RUN_RESUMED_AFTER_APPROVAL`,
  `TODO_*`, `RECOVERY_ATTEMPTED`.
- `proto/model_plane/v1/execution.proto` — `ExecuteStep` returns
  `awaiting_approval`; `ResumeRun` exists.

Already **implemented** (not stubs):

- `session-core/src/orchestration_grpc.rs` — serves `OrchestrationCoreService`
  incl. `StreamRunEvents`; backed by `session-core/src/orchestration_store.rs`.
- `rust/crates/mp-orchestration` — Plan/Todo/Approval/Lineage records + state
  validators (`approval.rs` has the full grant/deny/timeout machine).
- `model-gateway/src/sse.rs` — `invoke_stream_sse` (chat) and `run_events_sse`
  (`GET /v1/runs/{id}/events`, maps `OrchestrationEvent` → SSE).
- `execution-core/src/runtime_loop/mod.rs` — `execute_step` returns
  `awaiting_approval` when an approval is required.

So this is a **close-the-gaps** effort, not greenfield.

## 1. The two harness profiles

Verevon has two AI surfaces with different reliability shapes. They share one
runtime; only the *profile* (config swapped at runtime) differs — the
orchestrator never changes.

| Concern        | `chat` profile (ChatGPT/Claude/Manus-style)        | `deployed_agent` profile (Intercom/Chatbase/Zendesk-style) |
|----------------|----------------------------------------------------|------------------------------------------------------------|
| Audience       | One user ↔ their own AI                             | Bot serves the *customer's customer*; Verevon user = operator |
| Task graph     | **Invisible.** Harness stays hidden                | Operator-facing inbox + plan/approval surfaces             |
| HITL           | Optional inline approval for risky tools           | Mid-conversation human takeover (handoff)                  |
| Resumability   | Resume the answer stream on reload                 | Resume the operator's live event feed on reload            |
| Persistence    | Thread/run as today                                | + conversation status, `assigned_to`, tags                 |
| Feedback       | thumbs up/down (optional)                           | 👍/👎 → training/eval signal (skill promotion)             |

**Design**: profile is a field on the agent definition (`profile: "chat" |
"deployed_agent"`), read by the gateway/execution-core to decide which
lifecycle hooks and operator events fire. The clean chat UI is never touched by
operator concerns.

**Status** ✅ persisted + resolved (Phase 2): the field lives on the Convex
`agents` table (schema + `create`/`update` mutations) and the verevon
`PersistedAgent` type. `resolveAgentProfile(agent)` in
`components/agents/types.ts` applies the migration default — `deployed_agent`
when `publicEnabled`, else `chat`. `useAgentPlayground` exposes the resolved
`profile` so the workspace view branches (operator run-event panel for
`deployed_agent`, clean for `chat`). Remaining: the create/edit form control and
gateway/execution-core hook branching on profile.

## 2. Ask → contract → gap matrix

| Ask | Existing contract | Gap to close | Phase |
|-----|-------------------|--------------|-------|
| Two harness profiles | none | add `profile` to agent def; branch hooks/events on it | 2 |
| Full SSE resumability (resume on reload, lost final-chunk) | `invoke_stream_sse`, `run_events_sse` | no `id:` field, no `Last-Event-Id`, deltas not persisted | 1+2 |
| HITL primitive | `Approval` + `RunPaused/ResumedForApproval` + `DecideApproval` | verify execution-core creates Approval + emits pause event + blocks on resume | 2 |
| Auth/retry/telemetry not reinvented per route | `auth-token.ts` mints tokens | verevon `harness-client.ts` wrapper (token + retry + correlation-id + telemetry) | 1 |
| Plan structure server-side + authoritative task graph | `Plan`/`PlanStep` + `OrchestrationCoreService` | already authoritative; surface via hook | 2 |
| Common task-graph feed (no per-hook polling) | `StreamRunEvents` + `run_events_sse` | verevon `useRunEvents` hook + proxy route | 2 |
| "plus more" reliability | events, idempotency, OTEL | correlation-id end-to-end; recovery events; golden parity tests | 2+ |

## 3. SSE resumability protocol

Two layers, because the two streams differ:

### 3a. Run-event feed (`/v1/runs/{id}/events`) — operator + task graph ✅ IMPLEMENTED

Orchestration events ride an **in-memory broadcast channel** (no durable event
log), so resume = bounded replay buffer + snapshot fallback rather than event
sourcing:

1. Each `OrchestrationEvent` carries a `event_id` (ULID), assigned by
   session-core at broadcast time (`broadcast_event`). *(proto field added)*
2. session-core keeps a **bounded per-run ring buffer** (`REPLAY_BUFFER_PER_RUN
   = 256`) of recent events.
3. Gateway sets the SSE `id:` line to `event_id`. The browser's `EventSource`
   auto-sends `Last-Event-Id` on reconnect.
4. Gateway forwards it as `StreamRunEventsRequest.after_event_id`; session-core
   subscribes first (no gap), replays buffered events with `id > after_event_id`,
   then tails live (deduping the overlap by id).
5. **Snapshot fallback**: if the cursor predates the buffer (eviction / restart),
   the client reconciles via `ListPlans`/`ListTodos`/`ListApprovals`. The verevon
   `useRunEvents` hook owns this; the buffer is a best-effort fast path, the
   snapshot is the correctness backstop.

Wire path: browser `EventSource` → verevon proxy `/api/agents/runs/{id}/events`
(`harnessFetch`, forwards `Last-Event-Id`) → gateway `run_events_sse` → gRPC
`StreamRunEvents(after_event_id)` → session-core replay+tail.

### 3b. Chat answer stream (`/v1/invoke/stream`) — resume-on-reload

1. Each delta SSE event gets an `id:` = sequence index within the request.
2. Partial completion is buffered (Redis, keyed by `request_id`, TTL ~10 min)
   as deltas are produced.
3. New endpoint `GET /v1/invoke/{request_id}/resume` replays buffered deltas
   from `Last-Event-Id`, then tails or returns the final `done`.
4. Lost-final-chunk handling: client treats stream close *without* a `done`
   event as "incomplete" and calls resume rather than showing a truncated
   answer.

**Phase 1 lands**: the `id:` field on both streams (forward-compatible; browser
starts sending `Last-Event-Id` immediately, server ignores harmlessly until
replay ships). **Phase 2 lands**: the replay paths + Redis buffer.

## 4. HITL primitive (handoff + approval)

Both shapes are one underlying primitive: *suspendable run with external resume
signal*.

```
execute_step → needs approval/handoff
  → create Approval (kind=TOOL_CALL|DESTRUCTIVE|... or handoff)
  → run.status = awaiting_approval
  → emit RUN_PAUSED_FOR_APPROVAL  (→ StreamRunEvents → operator inbox / chat)
  → loop suspends
operator/user DecideApproval(granted|denied)   OR   handoff reply
  → emit APPROVAL_DECIDED + RUN_RESUMED_AFTER_APPROVAL
  → ResumeRun re-enters the loop with decision context
```

`chat` profile uses it for inline risky-tool confirmation; `deployed_agent`
profile uses it for Intercom-style mid-conversation human takeover.

## 5. verevon harness client (Phase 1, this change)

`src/lib/model-plane/harness-client.ts` centralizes what every route reinvents:

- Bearer token minting (cookie → internalClaims → env), deduping the block
  currently copy-pasted in `reasoning.ts` (twice).
- `x-correlation-id` generation + propagation (ties verevon → gateway → NATS).
- Retry with exponential backoff + jitter on retryable failures (429/502/503/
  504/network), honoring `Retry-After`. POST is retried only when explicitly
  marked idempotent.
- Timeout via `AbortController`, composing any caller signal.
- Pluggable telemetry reporter (status, latency, attempts, correlation id).
- Typed `HarnessError { code, status, correlationId, retryable }`.

## 6. Phased plan

- **Phase 1** ✅: this doc · `harness-client.ts` · SSE `id:` field on both
  gateway streams.
- **Phase 2** ✅: proto `event_id` + `after_event_id`; session-core bounded
  replay buffer; gateway `Last-Event-Id` wiring; verevon proxy + `useRunEvents`
  hook (with snapshot fallback); agent `profile` field end-to-end (Convex
  schema + mutations + verevon types + create/edit form selector); operator
  run-event panel in `AgentWorkspaceView` (deployed_agent only); chat
  resume-on-reload (in-memory delta buffer + `/v1/invoke/resume/{request_id}`
  + verevon `/api/chat/resume/{requestId}` proxy); profile → approval-posture
  policy (`gateway/src/profile.rs`) stamped on the STREAM_OPENED envelope and
  fed by verevon's chat-stream route.
- **Phase 3** ✅ (this round): HITL enforcement in execution-core's step loop
  (risky tool + `ask` posture → `CreateApproval` on session-core → row +
  `RUN_PAUSED_FOR_APPROVAL` broadcast → operator panel/snapshot show the pause;
  `DecideApproval` resumes); browser resume primitive (`chat-resume.ts`) with
  sessionStorage persistence wired into the chat stream lifecycle and the
  gateway request_id surfaced through the pipeline (`meta` SSE frame).
- **Phase 4** (in progress):
  - ✅ **Operator inbox UI** — `AgentInbox` (run list + detail), shown as a
    `deployed_agent`-only workspace tab; wires `useRunEvents` + HITL
    approve/reject. Backed by verevon proxies `GET /api/agents/runs` (lists
    `agentRuns:listForOrg`) and `POST /api/agents/approvals/{id}/decide`.
  - ✅ **Correlation-id golden parity tests** — `sse.rs` tests lock that every
    emitted envelope derives `correlation_id` from the request id, identically
    across HTTP/SSE and gRPC.
  - ✅ **Feedback → skill-promotion loop** (full Temporal workflow). End to end:
    verevon rate route → gateway `POST /v1/feedback` publishes `mp.v1.feedback.rated`
    → orchestrator-core subscriber folds it into a `FeedbackStore` →
    `FeedbackPromotionWorkflow` (nightly) runs `AggregateFeedbackActivity`
    (skills above sample + good-ratio threshold) → launches a child
    `SkillPromotionWorkflow` per candidate (validate → gate → registry). Feedback
    *nominates*; the existing promotion gates still *decide*. Tested with the
    Temporal `testsuite` (promote / gate-fail-skip / no-candidates) + a
    `FeedbackStore` unit test.
  - ✅ **Redis-backed buffers** (full implementation). Both the gateway
    stream-delta buffer (`stream_buffer.rs`) and the session-core run-event
    replay buffer (`orchestration_grpc.rs`) are now `Memory | Redis` enums
    selected by `REDIS_URL` at startup; a reconnect that lands on a different
    replica still resumes from the shared Redis list (TTL + LTRIM cap). Falls
    back to in-memory if `REDIS_URL` is unset or Redis is unreachable. Events
    are prost-encoded + base64 in Redis; the broadcast hot path spawns the
    Redis write so it never blocks. Existing buffer tests still pass.

### HITL enforcement chain (implemented)

```
execution-core execute_step
  → permission::evaluate(Ask, risky tool) = AwaitApproval
  → status "awaiting_approval"
  → OrchestrationCoreService.CreateApproval (run_id, step_id, kind=DESTRUCTIVE)
session-core create_approval
  → store::request_approval (durable row, state=requested)
  → broadcast ApprovalStateChanged(REQUESTED) + RunPausedForApproval
  → StreamRunEvents → gateway SSE → verevon useRunEvents → operator panel
operator decides
  → DecideApproval(granted|denied) → ApprovalStateChanged → run resumes
```

`profile.rs` maps `deployed_agent → ask` / `chat → auto`; benign reads proceed
under `ask`, only risky/destructive tools gate (`permission::is_risky_tool`).

### Chat resume mechanism (implemented)

Partial answers persist to Convex during streaming, so the bulk survives reload
via the provider's reactive reload. The gateway delta buffer + `/v1/invoke/
resume/{request_id}` recover the unflushed tail and the final `done`
(lost-final-chunk). The browser remembers the gateway `request_id`
(surfaced as a `meta` SSE frame) in sessionStorage and `resumePendingStream`
replays it; the marker clears on normal completion.

## 7. Verification gates

- `cargo check --workspace` + `cargo test --workspace` green after each Rust change.
- `pnpm typecheck` green after verevon changes.
- SSE: kill the browser tab mid-stream, reload → stream resumes from
  `Last-Event-Id` with no duplicated or dropped deltas.
- HITL: a destructive tool call pauses the run, surfaces an approval, and only
  proceeds after `DecideApproval(granted)`.
