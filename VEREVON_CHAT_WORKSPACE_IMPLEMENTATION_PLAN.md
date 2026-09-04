# Verevon Chat Workspace — Start-to-Finish Implementation Plan

**Status:** Backend wiring checkpoint complete — A2A provider contract remains gated  
**Date:** 2026-08-30  
**Target:** `apps/Frontend Plane/verevonv3` and its same-origin gateway/runtime dependencies  
**Frontend:** Solid 2, TypeScript 7, Vite 8.2.2  

### Implementation checkpoint — 2026-08-30

The first end-to-end slice is now implemented in `verevonv3`: contextual chat surfaces stay beside the transcript; attachments are rendered as honest ephemeral viewers; attachment chips can summon the right-hand canvas and select the clicked file without replacing the conversation; durable plans, todos, and delegation lineage are read from the orchestration API; normalized Verevon UI events feed the Work panel; live run streams record SSE cursors with an explicit reconnect path; and a compact proof/receipt summary is available from the existing run proof-bundle endpoint. The composer now exposes a calm context inspector for internal knowledge, web, research, and session-only attachments, makes the existing read-only/approval-gated boundary visible as Ask/Do, and internal source cards fall back to the existing Knowledge surface when no source URL is supplied. Do runs now summon Work at the authoritative `connected` event instead of waiting for a later step. A dedicated Trace canvas now combines the proof bundle with the safe normalized event timeline, without exposing hidden reasoning or provider credentials. Agentic run ids and Ask/Do mode are now carried through the local/server transcript snapshot, so a reload can reconnect to the same durable run; the live panel checks the authoritative run status first and does not label an already terminal run as active. The selected contextual canvas tab is now remembered per durable thread (never for temporary or foreign-origin threads), so returning to a conversation restores the user’s focused PDF, Sources, Work, Output, or Trace surface without changing routes. Work tool calls resolve from that same durable run id, preventing a later follow-up from leaking its tools into an older run’s panel. CSV attachments now open as a bounded, safe table preview with a text fallback for empty or unsupported data. The chat ingress now also adapts the current AG-UI lifecycle, text, tool, reasoning, state, activity, and sub-agent event vocabulary into the same Verevon-owned event projection while leaving unsupported raw payloads non-mutating; allowlisted AG-UI CUSTOM envelopes (artifact, attachment, citation, grounding, step, tool result, usage, and reasoning delta) now drive the same focused callbacks as native events. The gateway AG-UI route now keeps the durable orchestration run id separate from the invocation/request id, runs through the same Space/support/ZDR enrichment and delegated-audience checks as native chat, forwards all governed request controls, retains upstream SSE cursor ids when one native frame expands into multiple AG-UI frames, and exposes a cursor-aware AG-UI replay route for reconnecting runs. AG-UI `RUN_FINISHED` interrupt outcomes are now projected as canonical resumable pauses rather than successful completion in both the AG‑UI-only and native chat adapters, preserving approval/Work state without a duplicate terminal callback. Vite is pinned to the requested 8.2.2 line. Basic verification currently passes (`pnpm typecheck`, `pnpm build`, the focused AG-UI client suite, gateway `cargo check` plus the four AG-UI gateway unit tests, and a preview `/chat` smoke request returning HTTP 200); the preview shell still reports expected gateway 502s when the gateway is not running. The remaining phases below are still roadmap work and are not being treated as complete until their backend contracts and browser behavior are verified.

Current UI invariant follow-up: contextual tabs are evidence-driven. Ordinary Ask turns do not expose empty Sources, Artifacts, or Steps destinations, and a stale persisted tab fails closed to Chat. CSV attachments open as a bounded, safe table preview with text fallback for empty or unsupported data. Work/Trace proof summaries now expose owner-issued receipt identifiers and verification states from the durable proof bundle instead of only aggregate counts. Non-image attachments are now labelled preview-only in the composer/context inspector and attachment canvas; Chat exposes an explicit "Add to knowledge" action that converts the current in-memory file bytes to the existing multipart Ingestion Plane import route, while temporary/ZDR chat blocks the durable action and imports-core remains the final enforcement point.

Work-canvas follow-up: when a durable run is active and the user selects Steps, the live browser/run panel becomes the single Work canvas host and embeds the plan, proof, tool-call, context, and normalized event projections inside that run-owned panel. This removes the previous two-panel composition for the primary Work destination while preserving the live stream state across tab changes.

Reload-correlation follow-up: opening a durable thread now makes one bounded read of the authoritative `/api/v1/agents/runs?thread_id=…` endpoint when transcript metadata has no run link. A plan run is attached only when its server-owned mode and normalized goal match the newest assistant exchange; otherwise no run or Work surface is invented. Ordinary Ask runs keep their ids for receipts/feedback but are filtered out of the live Work rail, so the base chat remains calm. Deep-research turns remain eligible for Work through their explicit research tool mode.

Canvas ownership follow-up: Sources, Output, and Trace now hide the mounted live-run watcher from layout instead of rendering a second rail. The watcher remains subscribed so switching back to Work or Chat does not restart the run stream or lose its cursor.

Grounding follow-up: the composer now keeps the active grounding scope visible beside the context control (Knowledge by default, Knowledge + web when web search is enabled), while the existing inspector continues to explain the effective sources and research/attachment state. The explicit knowledge-upload affordance is shown only for formats accepted by imports-core (PDF, DOCX, TXT, Markdown, CSV, JSON, and HTML); image attachments remain model inputs and are not offered as unsupported durable documents.

Accessibility follow-up: opening Sources, Output, Trace, or the non-live Work canvas now moves focus into the mounted tabpanel body (programmatically focusable, outside the normal tab order) and labels the panel through its heading. This makes the contextual rail a real keyboard/screen-reader destination while leaving the transcript and live stream mounted.

Attachment-viewer follow-up: the file selector now exposes real tab/panel relationships (`aria-controls`, `aria-labelledby`, and a focusable panel) so switching between PDF, image, text, and CSV previews remains understandable to keyboard and assistive-technology users.

Preview-isolation follow-up: generated and attached HTML previews now share a small CSP wrapper with `connect-src 'none'`, no external scripts/assets, no forms, no child frames/workers, and `referrerpolicy="no-referrer"`; the existing `sandbox="allow-scripts"` remains without `allow-same-origin`.

Control-boundary follow-up: the global Chat composer no longer presents an `@` people autocomplete backed by the knowledge-search endpoint. Global Chat has no verified Control-owned roster/agent-ref contract; Space rooms retain their governed `@` agent/person picker and send `mentionedAgentRef` only after a roster selection. Typing `@` in global Chat remains ordinary text instead of a misleading invocation control.

Approval-stream follow-up: the run-owned Work watcher now treats approval state, approval pauses/resumes, and browser-action approval events as refresh signals for the owning assistant turn. It re-reads pending approvals through the existing orchestration client (rather than synthesizing cards from SSE payloads), so a pause recovered from the durable run replay can surface the same approval controls without requiring the chat message stream to still be connected.

AG-UI governance follow-up: the gateway now carries existing approval/pause/browser lifecycle events as non-executable AG-UI CUSTOM envelopes, and the Verevon event adapter projects only an explicit allowlist into typed pause/resume/approval/verification entries (with compatibility step callbacks). Native lifecycle names cannot be smuggled through `CUSTOM`; arbitrary custom names remain opaque and non-mutating.

Agentic boundary follow-up: the native Model Plane `awaiting_approval` frame is now projected into the same waiting-step and authoritative approval refresh path as `run_paused_for_approval`, including the AG‑UI adapter route.

Thread-rail status follow-up: Session Core’s existing `latest_run_id`, `latest_run_status`, and `latest_run_updated_at` projection now survives the Chat gateway and Solid client. The history switcher shows only quiet queued/running or attention states (approval/paused/failed); it does not poll every thread or open a Work canvas for completed runs.

Durable Trace replay follow-up: the Model Gateway now exposes a read-only `/v1/threads/:thread_id/events` route backed directly by Session Core's `ReplayThread` RPC, and the Frontend Gateway proxies it at `/api/v1/chat/threads/:thread_id/events`. The browser receives only event-envelope metadata (id, type, schema version, timestamp, producer, correlation, and resource reference); protobuf `Any` payloads, tenant identifiers, and provider data stay server-side. On a durable Work-thread reload, Chat walks the cursor pages (500 events per request, 5,000 rendered-event safety cap), de-duplicates ids, and projects the result into the Trace canvas; an in-flight waiting stream is skipped so its resumable SSE buffer remains the sole live source. A cap is surfaced as an explicit truncation notice rather than silently implying a complete audit. This closes the first durable-history path while leaving a future server-side retention/export contract and richer typed-event reduction as separate work.

Durable browser replay follow-up: browser-agent action/observation/pause/resume/approval/verification events emitted through the existing `RecordOrchestrationEvent` RPC are now persisted as an allowlisted, reference-only projection in Session Core's canonical event log. Model Gateway exposes `/v1/runs/:run_id/events/replay`, authorizes the run first, filters the `ReplayThread` stream back to that run, and returns only the safe browser payload plus a page cursor; the Frontend Gateway carries the existing model/session audiences at `/api/v1/runs/:run_id/events/replay`. The Work watcher hydrates those pages before opening the live SSE tail and resumes with the canonical cursor, so a completed browser run can restore its step timeline and artifact frames after reload without exposing DOM bytes, provider credentials, or a second browser transport. The live stream remains authoritative for new events; persisted replay is a return-after-navigation path, not a replacement gateway.

Cancellation-receipt follow-up: authorized run cancellation now calls Session Core's existing `RunService.CancelRun` instead of relying on a gateway-only NATS request notification. Session Core atomically flips the run to `cancelled` and appends a `RUN_CANCELLED` event; the returned event id is a durable receipt and is safe to return on idempotent retries. Model Gateway preserves the legacy `RUN_CANCEL_REQUESTED` fan-out as an optional notification, but no longer treats notification delivery as the status authority. Chat's Stop action cancels a durable Work run when one is active and records the confirmed receipt in the Trace projection; temporary/ZDR turns remain local and never call the durable mutation.

Typed-pause follow-up: the Solid event adapter now carries an explicit `blocked` / `approval` / `ambiguous` pause kind when the server event provides that evidence (approval pauses are classified by their authoritative event family; browser pauses are classified only from an explicit reason/status). The Trace label keeps the kind visible without turning an untyped pause into a guessed policy state. Missing evidence remains untyped, so the client does not manufacture a pause reason.

Effect-evidence follow-up: the existing Proof Bundle HTTP projection now exposes a server-derived `effect_class` without adding a second gateway or trusting model text. It is `external_receipt` only when a provider receipt is durable, `effectful` when an execution receipt exists without a provider receipt, `proposed_effect` when approval exists without execution, `read_only` when the bundle proves no approval-chain execution, and `unknown` when no run proof exists. The Chat Work/Trace proof summary renders that classification; it deliberately does not claim that a plan or approval alone changed the outside world.

Steering follow-up: the existing browser-run control proxy is now reachable from the contextual Work panel once real browser-frame evidence exists. Pause/resume controls wait for the durable browser lifecycle event before changing the local state, preserving a soft-interrupt boundary; queued mid-run guidance remains the queue path, and the composer Stop action remains the hard-stop/cancellation-receipt path. Non-browser runs do not gain misleading browser controls.

Effect-action follow-up: server-derived `effect_class` is now carried onto
run-linked assistant turns and persisted in the local/server transcript snapshot.
The chat action layer enforces the immutable-effect boundary: documented
effectful/external-receipt turns no longer offer in-place regenerate, and the
preceding user turn no longer offers in-place edit. The controller repeats the
guard so keyboard/automation paths cannot bypass the boundary; the UI directs
the user to start a new turn or inspect Trace for the durable receipt. Missing
or unavailable proof remains unclassified and is never silently treated as
read-only. Effectful turns also expose a real “run as new turn” action that
reuses the original user request through the normal composer/send path, leaving
the original receipt-bearing exchange untouched. Branching from an effectful
assistant now uses the preceding user turn as the branch boundary, so the new
chat does not inherit a side-effect-bearing answer as if it were new context.

Replay-label correctness follow-up: the durable thread Trace projection now
resolves legacy thread/message/step/cancellation records from their owning
protobuf `Any.type_url` before falling back to the envelope enum. This avoids
mislabeling `THREAD_CREATED`, `MESSAGE_APPENDED`, and `STEP_COMPLETED` as
unrelated plan/action events when Session Core replays older rows, while still
keeping all payload bytes, tenant identifiers, and provider data server-side.

Origin-link follow-up: Space activity rows now link back to their owning
`/spaces/:space_id` route (with the thread as a contextual query), and Support
Verevon rails link back to the authorized Support conversation. New owner-plane
links no longer adopt Space/Support threads into `/chat`; the Chat read-only
foreign-origin path remains solely for legacy deep links.

Thread-rail status follow-up: the server-owned `latest_run_id`,
`latest_run_status`, and `latest_run_updated_at` fields now survive the local
history normalization/carry path instead of being discarded when a routine
snapshot refreshes a title or preview. The Core sidebar renders only queued,
running, paused/approval, blocked/ambiguous, or failed hints; completed and
cancelled runs remain visually quiet. This preserves the calm base chat while
making attention-worthy work discoverable without polling every thread.
During a live durable turn, the controller also publishes a presentation-only
local hint on connect/pause/resume/complete/failure/stop so the sidebar reacts
through its existing history-change event; durable controls continue to read
the server-owned run and proof endpoints.

AG-UI protocol audit follow-up (2026-08-30): the bridge now emits the AG-UI
base `timestamp` and adapter metadata on every mapped frame, uses the
protocol's discriminated `RunFinished.outcome` shape while retaining the
native terminal status as additive result data, and serializes tool arguments
as streamed `ToolCallArgs.delta` fragments. Unknown native frames are exposed
as `RAW` passthrough events (with a source label) rather than being presented
as executable-looking `CUSTOM` events. The Solid client keeps timestamp,
metadata, raw/source, snapshot/delta, outcome, and parent-correlation fields
available to callers and accepts both structured and streamed tool arguments.
This follows AG-UI's event categories, required run lifecycle boundaries,
metadata rules, and RAW-versus-CUSTOM distinction; Verevon's own event model
and governance callbacks remain authoritative. See the [AG-UI event
reference](https://docs.ag-ui.com/concepts/events).

A2A protocol audit follow-up (2026-08-30): the official A2A material confirms
that discovery/Agent Cards, messages and artifacts, and long-running Task
status are the interoperable boundary. The Rust implementation is available
through the official [`a2a-rs`](https://github.com/a2aproject/a2a-rs) workspace
(`a2a`, `a2a-client`, and `a2a-server` crates; current SDKs require Rust 1.85+),
but this checkout still has no approved provider endpoint, server-owned Agent
Card, registry row, or authorized task route. Phase 9 therefore remains an
explicit backend contract spike: do not add a fake remote-agent picker or
route Space agents through an unverified A2A façade. When a provider is
approved, use the Rust SDK at the Model/Agent boundary, introduce the
Application-owned registry and Control authorization first, then map A2A Task
events into nested Verevon delegation activity with parent/child correlation,
bounded authority, cancellation, receipts, and artifact validation. See the
[A2A technical documentation](https://agent2agent.info/docs/), [A2A reference
repository](https://github.com/a2aproject/A2A), and [official Rust
workspace](https://github.com/a2aproject/a2a-rs).

Surface-registry follow-up (2026-08-30): contextual destinations now share a
single allowlisted Solid contract in `features/chat/lib/chat-surfaces.ts`.
The registry owns tab ids, labels, descriptions, icons, precedence, and
evidence-driven availability; `ChatTabs`, `ChatWorkspaceCanvas`,
`ChatPage`'s stale-tab guard, and per-thread tab restoration all consume that
contract. Adding a new PDF, browser, file, table, HTML, Sources, Work, or
Trace destination therefore requires an explicit registry entry rather than
another independent visibility switch.

Summon-precedence follow-up (2026-08-30): automatic evidence handoffs now use
the registry priority (`Work > Output > Sources > Trace`). A lower-priority
surface can be replaced only while the current tab was itself automatically
summoned; once the user selects a tab, later events preserve that choice. This
keeps first-summon behavior deterministic without making the contextual canvas
fight the user for focus.

Citation-contract follow-up (2026-08-30): the current Model Plane citation
payload is source-level (`id`, title, URL, snippet) and the retrieval prompt
numbers evidence as `[n]`; it does not yet return authoritative character
ranges, claim ids, or source-group bindings. The UI now promotes only explicit,
in-range `[n]` markers to a compact, keyboard-accessible source chip and keeps
unknown markers as text; it never heuristically attaches a source to an
arbitrary sentence. Phase 6's full Perplexity-style contract remains open:
add server-issued claim anchors/source groups (or a typed answer-part
structure), validate that every anchor references an authorized citation, then
render grouped domain chips and a 1/N verification popover. Until that
server-issued contract exists, the Sources canvas remains the authoritative
fallback for unmarked or ambiguous evidence.

Schema-ownership follow-up (2026-08-30): durable cross-plane event envelopes
remain owned by `apps/Model Plane/proto/model_plane/v1/events.proto`; Session
Core and Model Gateway use that envelope for replay and authorization. The
frontend `VerevonUiEvent` union is intentionally a browser projection, not a
second durable schema, and AG-UI is an adapter at the gateway boundary. The
remaining Phase 2 task is to generate or validate the shared Rust/TypeScript
discriminants from the proto/adapter contract so new event families cannot
drift silently; do not add a parallel hand-written gateway event taxonomy.

AG-UI round-trip follow-up (2026-08-30): canonical Verevon events emitted by
the reverse adapter now have an explicit, non-executable `CUSTOM` allowlist on
ingress. Generic pauses (`run_paused`) and dotted canonical event names carry
their discriminant through the value and round-trip back to the same typed
event; arbitrary custom names remain opaque. `RUN_ERROR` projections also keep
their run/request correlation ids for trace scoping.

Citation-contract readiness follow-up (2026-08-30): the browser citation
projection now preserves optional server-issued `claimId`, `sourceGroupId`,
`start`, and `end` fields across native SSE, AG-UI CUSTOM, transcript
normalization, and the shared `Citation` type. No client-side claim is inferred
when those fields are absent; the existing explicit `[n]` marker rule remains
the only promotion path until Model Plane emits and authorizes claim anchors.
Canonical dotted CUSTOM events emitted by the reverse adapter now dispatch
through the same focused artifact, attachment, citation, grounding, step,
usage, title, follow-up, queue, and lifecycle callbacks as their legacy aliases.

Verification note: the latest `pnpm typecheck` and full `pnpm build` pass on Vite 8.2.2 after the canvas focus, global-mention boundary, durable plan-run hydration, calm Work-rail, attachment-tab accessibility, HTML preview CSP, thread-rail status, durable Trace replay, durable browser replay, cancellation receipts, typed-pause projection, effect-action enforcement, fresh-turn rerun, safe branch-boundary changes, replay-label correction, owner-link correction, AG-UI compatibility updates, AG-UI interrupt preservation, explicit citation-marker rendering, surface registry, summon precedence, and AG-UI canonical-event round-trip protection. Model Gateway, Session Core, and Execution Core all pass Windows `cargo check`; the execution-core shutdown handler now uses Ctrl-C on non-Unix targets. Session Core's orchestration unit suite passes (41/41), including the browser-event replay/buffer coverage; the model-gateway durable-trace projection tests pass 2/2, and gateway history tests remain 7/7. Gateway AG-UI unit coverage passes 4/4, the isolated AG-UI client suite passes 7/7, and the markdown/citation parser suite passes 14/14 with a single worker. The gateway security/action/product-truth contract checks pass 7/7. The default Vitest worker can still time out before client tests start on this Windows checkout, so constrained single-worker commands are the reproducible checks; this is a test-runner reliability issue rather than a source/build failure.

Verification addendum (2026-08-30): after the citation-readiness, canonical-CUSTOM dispatch, and stopped-run lifecycle changes, `pnpm typecheck` passes; the isolated AG-UI client suite is 9/9, the chat client suite is 46/46, and the markdown/citation parser suite is 14/14. The combined multi-file Vitest invocation can still time out one Windows worker, so these single-file runs remain the reproducible checks.

Backend checkpoint addendum (2026-08-30): the current checkout also passes the
Solid 2 compatibility check for the fleet console, `pnpm build` on Vite 8.2.2,
gateway `cargo check`, and Model Gateway `cargo check`. The local Model Plane
HTTP health/readiness endpoints return `200`; the frontend gateway was not
running during this check, so no unauthenticated chat response was claimed.

Runtime audit checkpoint (2026-08-30): the local Model Plane Docker stack is up, with Model Gateway HTTP health/readiness returning `200 ok` on `127.0.0.1:8080`; its gRPC listeners are present on `9090` and `9092`. The protected `/v1/models` route returns `401` without a bearer, and the running container reports `MODEL_GATEWAY_AUTH_DEV_BYPASS=0`, so no unauthenticated chat smoke was fabricated. A verified user/session bearer is still required for an end-to-end transcript or streaming request. This is an intentional release boundary, not a frontend failure.

## 1. Product decision

Verevon begins as a calm, grounded conversational assistant and expands into a persistent work environment only when the task produces work worth inspecting.

The product should feel like ChatGPT, Claude, Perplexity, Manus, and NotebookLM during normal conversation. As tasks become longer or produce material outputs, it should gain the persistence and control associated with T3 Code, Claude Code, Cursor, DeepSeek Harness, and Antigravity—without becoming a code editor.

The governing principle is:

> **IDE-like capability, not IDE-like cold-start UI.**

Conversation remains the command center. The workspace grows around it through contextual surfaces such as Sources, Work, Output, Trace, PDF, browser, HTML preview, file preview, tables, revisions, approvals, and receipts.

## 2. Architectural decision

Verevon will own its UI, runtime, event model, and governance semantics.

- **assistant-ui** is a behavioral and component blueprint. Its React components are not installed in the Solid application.
- **AG-UI** informs the agent-to-user event vocabulary and becomes an interoperability adapter at the gateway boundary.
- **A2A** is used behind the UI for agent discovery, delegation, long-running tasks, artifacts, and status exchange.
- **MCP and Verevon action contracts** remain the agent-to-tool/data boundary.
- **Solid 2 primitives** remain the only browser rendering/reactivity model.

### Protocol and blueprint references

- [assistant-ui](https://github.com/assistant-ui/assistant-ui) — interaction and component patterns to study, not a runtime dependency.
- [AG-UI introduction](https://docs.ag-ui.com/introduction) and [event reference](https://docs.ag-ui.com/concepts/events) — interoperable, event-based agent/user streaming vocabulary.
- [A2A documentation](https://agent2agent.info/docs/) and [A2A reference implementation](https://github.com/a2aproject/A2A) — discovery, delegation, task status, and artifact exchange behind the gateway.

```text
Solid workspace
  └─ Verevon UI runtime
      ├─ thread/message/composer state
      ├─ contextual surface registry
      ├─ approval and receipt policy
      └─ typed event reducer
          └─ Verevon UI Event v1
              ├─ existing Verevon SSE adapter
              ├─ AG-UI adapter
              └─ run/delegation projection
                  ├─ Verevon agents
                  ├─ browser/document/research agents
                  └─ A2A-compatible agents
```

## 3. Non-negotiable product invariants

1. A simple question produces a simple chat experience.
2. No panel opens without a task event that justifies it.
3. Only one contextual surface is focused at a time.
4. Cross-surface records remain owned by their original surface.
5. Effectful turns are immutable and always produce durable receipts.
6. Ask is read-only by default; Do is a deliberate escalation.
7. Grounding scope is visible and governed by tenant policy.
8. Stream replay reconstructs the same state as uninterrupted delivery.
9. The browser never receives raw model/provider credentials or forged scope.
10. Agents may request only allowlisted native UI surfaces; they never send executable UI code.
11. Closing or navigating away from a running task is safe.
12. ZDR and retention policy propagate through events, artifacts, traces, caches, and local storage.

## 4. Workstreams

The project is delivered through four coordinated workstreams:

| Workstream | Responsibility |
|---|---|
| Product and interaction | Vocabulary, progressive disclosure, Ask/Do, panel rules, mobile behavior, accessibility |
| Solid UI platform | Runtime, primitives, render registry, workspace shell, viewers, performance |
| Protocol and backend | Canonical events, SSE replay, AG-UI adapter, A2A delegation, durable runs and receipts |
| Trust and quality | Origin enforcement, permissions, retention, tests, observability, staged rollout |

No UI phase may bypass the protocol and trust workstreams with local-only state that represents durable agent work.

## 5. Phase 0 — freeze the product contract

### Objectives

- Convert the current design thesis into explicit, testable interaction rules.
- Resolve terminology before new components are built.
- Establish current performance, accessibility, and reliability baselines.

### Actions

1. Approve the primary information architecture:
   - Left: personal threads and live-work status.
   - Center: conversation and inline work blocks.
   - Right: one contextual canvas with Work, Output, Sources, and Trace.
2. Approve deterministic surface precedence: `Work > Output > Sources > Trace`, then recency within a surface.
3. Define when each surface is summoned and when it may take focus.
4. Set Norwegian and English labels for Ask/Do, Work/Output/Sources/Trace, grounding, typed pauses, receipts, and autonomy budgets.
5. Decide whether Trace is an audit record, a sharing feature, or two separate products.
6. Define mobile behavior: the contextual canvas becomes a full-screen sheet; the conversation is never squeezed into an unusable column.
7. Record baseline metrics:
   - time to first token;
   - stream update rate and dropped-frame rate;
   - memory use for long threads;
   - reconnect success rate;
   - task return/resume success;
   - keyboard and screen-reader completion;
   - simple-question completion without opening a panel.
8. Inventory assistant-ui interaction patterns and tests. Record provenance for any substantially adapted MIT-licensed code.

### Exit gate

- Product invariants and terminology are signed off.
- Every contextual surface has a trigger, focus rule, dismissal rule, and mobile rule.
- Baseline measurements are reproducible.

## 6. Phase 1 — close correctness and governance leaks

This phase precedes visual redesign because a polished workspace must not make unsafe behavior easier to reach.

### Actions

1. Complete thread-origin enforcement:
   - retain the server-side `origin == chat` listing rule;
   - remove Support, Inbox, Spaces, Ticketing, Agents, Knowledge, and Ingestion links that route foreign work into `/chat`;
   - add a lint/contract test forbidding new cross-surface `/chat` adoption links.
2. Handle legacy foreign-thread deep links:
   - load only after authorization;
   - render a read-only ownership banner;
   - link back to the owning surface;
   - disable the composer and all mutating message actions;
   - never add the thread to chat history.
3. Introduce a first-class turn effect classification:
   - `read_only`;
   - `proposed_effect`;
   - `effectful`;
   - `external_receipt`.
4. Enforce action behavior:
   - read-only: copy, regenerate, edit, branch;
   - effectful: view receipt, rerun as a new turn, branch from before the effect;
   - never edit or regenerate an effectful turn in place.
5. Consolidate pin ownership on the server.
6. Complete non-image attachment honesty:
   - either connect the existing chat-document upload route end to end;
   - or temporarily restrict the picker to the file types actually transmitted.
7. Remove or complete dead inputs such as unbound mentions and unsupported modes.
8. Add contract tests for origin, ZDR, foreign-thread read-only behavior, receipts, attachments, and effectful message actions.

### Exit gate

- No non-chat thread can be adopted or mutated from chat.
- Every external effect has a durable, addressable receipt.
- The UI never advertises an attachment or mode that the wire silently drops.

## 7. Phase 2 — define Verevon UI Event v1

### Objective

Replace feature-by-feature stream wiring with one canonical, replayable event model that can represent current Verevon behavior and map cleanly to AG-UI.

### Event families

1. Run lifecycle: started, paused, resumed, stopped, failed, completed.
2. Message lifecycle: created, content delta, completed.
3. Reasoning summary: started, summary delta, completed; never raw hidden chain-of-thought.
4. Tool lifecycle: proposed, started, argument delta, result, failed.
5. State projection: snapshot and keyed higher-sequence-wins delta.
6. Activity: plan, step, todo, progress, checkpoint.
7. Evidence: citation, grounding, source group, provenance.
8. Artifact: created, revised, preview-ready, failed.
9. Approval/interrupt: requested, accepted, denied, guidance added, expired.
10. Steering: queued, delivered, soft-interrupted, hard-stopped.
11. Delegation: agent discovered, delegated, progress, artifact, returned, failed.
12. Usage and receipt: latency, tokens, cost, policy decision, effect receipt.
13. Terminal error: in-band, typed, actionable, and zero-usage safe.

### Envelope requirements

Every event carries:

- event id and monotonically comparable sequence;
- protocol version;
- org, user, thread, run, turn, and optional parent-run correlation;
- timestamp;
- origin and surface ownership;
- retention/ZDR posture;
- event type and typed payload;
- replay/idempotency metadata;
- visibility classification: user, audit, internal, or redacted.

### Actions

1. Define the TypeScript and Rust event schemas from one source of truth.
2. Map all current chat SSE variants into the new model.
3. Build an adapter from AG-UI events to Verevon UI Event v1 and the reverse mapping for supported public events. **Current frontend ingress:** uppercase AG-UI lifecycle/text/tool/reasoning/state/activity/sub-agent events are projected into the Verevon event stream at `chat-client.ts`; raw/custom events remain non-mutating and observable through the unknown-event path. The AG-UI gateway bridge now emits standard base metadata, lifecycle outcomes, streamed tool-argument fragments, and RAW passthrough for unknown native frames. `toAgUiEvent` provides a pure reverse serializer for supported public Verevon events; a generated/shared Rust/TypeScript schema remains open work.
4. Preserve custom Verevon events for governance, receipts, grounding, and tenant policy.
5. Make terminal errors in-band and ensure every run terminates exactly once.
6. Add protocol compatibility tests, golden fixtures, sequence tests, and unknown-event forward-compatibility tests.

### Exit gate

- An uninterrupted stream and a replayed stream reduce to byte-equivalent user-visible state.
- All existing chat features render through the canonical event contract.
- AG-UI compatibility is demonstrable without making AG-UI the product’s internal authority.

## 8. Phase 3 — make streams durable and thread-scoped

### Actions

1. Persist the full canonical event stream, not only answer text.
2. Scope resume state by durable thread and run identity.
3. Record the cursor while the live stream is arriving.
4. Resume citations, grounding, tools, approvals, artifacts, usage, receipts, and terminal state.
5. Make reconnect idempotent using event id plus sequence.
6. Keep producer work alive after the browser disconnects.
7. Add explicit resume outcomes: resumed, already complete, expired, unauthorized, unavailable.
8. Store live-run identity durably per thread so navigation and reload do not lose the job. **Implemented in the current slice:** `runId`, `planMode`, and the granted autonomy rung survive local/server transcript snapshots; reload hydrates pending approvals and checks authoritative run status before opening a live tail. Session Core’s canonical event log is now reachable through the read-only Model Gateway replay route, and Chat walks its cursor pages into the Trace canvas. Browser-agent progress emitted through `RecordOrchestrationEvent` is additionally persisted as a safe allowlisted projection and rehydrated through `/v1/runs/:run_id/events/replay` before the live cursor attaches, restoring browser evidence after reload. Full typed state reconstruction (rather than the safe envelope-only Trace projection) remains a separate contract because event payloads stay server-side.
9. Add multi-tab ownership tests so one thread cannot claim another thread’s stream.
10. Add TTL, size, backpressure, and compaction policies.

### Exit gate

- Refreshing, switching threads, closing the tab, and reconnecting never duplicate or lose visible work.
- Several threads may run concurrently without crossing events or cursors.

## 9. Phase 4 — build the Solid-native UI primitive layer

### Objective

Recreate the useful assistant-ui contracts as Verevon-owned Solid components and state machines.

### Primitive groups

- Runtime: `VerevonRuntimeProvider`, thread runtime, event reducer, command dispatcher.
- Thread: root, viewport, message list, scroll anchor, empty state, status announcer.
- Message: root, parts, content, reasoning summary, citations, action bar, branch picker, receipt.
- Composer: input, attachments, actions, grounding scope, effort, Ask/Do, queued steering.
- Work: work block, plan, step, tool, approval, typed pause, sub-agent.
- Workspace: surface host, surface tabs, focus controller, resize controller, mobile sheet.
- Thread rail: thread item, live status, attention state, unread result, pin/archive actions.

### Solid implementation rules

1. Store turns keyed by stable id; never replace the whole transcript for a token delta.
2. Update streamed text at the text-node signal level.
3. Use keyed `<For>` rendering for messages, parts, activities, sources, and artifacts.
4. Keep expensive derived collections in `createMemo`.
5. Give each thread an isolated runtime owner and abort lifecycle.
6. Keep persistent composer state outside surface switching.
7. Normalize server events before they reach components.
8. Register message and surface renderers by discriminated event/part type; avoid a growing central switch.
9. Virtualize only after measurement proves it is required; preserve stable scroll anchoring.
10. Provide keyboard and screen-reader behavior as part of every primitive contract.

### Exit gate

- The existing chat can render using the new primitives with no feature regression.
- Streaming remains smooth under the agreed stress profile.
- The primitive layer has unit, interaction, keyboard, and accessibility tests.

## 10. Phase 5 — deliver the familiar base chat

### Actions

1. Rebuild the default state around a calm central transcript.
2. Preserve streaming text, markdown, code, copy, feedback, safe branching, stop, and scroll behavior.
3. Make errors local and actionable:
   - answer failure;
   - rating failure;
   - attachment failure;
   - resume failure;
   - policy denial.
4. Make temporary/ZDR mode visible and explain its consequences.
5. Replace raw model choice with Quick, Standard, and Deep effort choices; keep provider/model transparency available on demand.
6. Keep right-side workspace and developer telemetry absent for ordinary Ask turns.
7. Complete keyboard navigation, focus restoration, tab semantics, reduced motion, 200% zoom, high contrast, and mobile reflow.

### Exit gate

- A first-time user can ask and continue a normal conversation without encountering workspace complexity.
- Base chat reaches the agreed accessibility and performance gates.

## 11. Phase 6 — evidence and Notebook-style grounding

### Actions

1. Add a persistent composer control showing the active grounding scope.
2. Default to the permitted internal corpus; expose web as an explicit additive choice.
3. Represent tenant restrictions as locked controls with explanations.
4. Bind citations to claims in the answer only after the backend emits
   authoritative claim anchors (claim id or character range) and source-group
   references. Validate those references against the authorized citation set;
   until then, render source-level cards without inventing sentence support.
5. Distinguish internal, uploaded, workspace, and external-web evidence.
6. Add source cards with provenance, freshness, permissions, and retrieval confidence.
7. Summon Sources on the first meaningful citation; do not repeatedly steal focus.
8. Add source preview and open-in-owning-surface behavior.
9. Make low-confidence and conflicting-source states explicit, including an
   abstain/re-query or “weak sources” outcome when retrieval cannot support a
   claim.

### Exit gate

- Users can tell what corpus was searched, which claims are supported, and whether external web was used.
- Unauthorized source details never leak through previews, citations, or traces.

## 12. Phase 7 — introduce the contextual workspace canvas

### Surface registry

Implement one allowlisted surface registry with these initial renderers:

- PDF/document viewer;
- browser session;
- HTML page preview in a sandboxed frame;
- generated document;
- image/media preview;
- spreadsheet/table;
- file preview/download;
- artifact revision comparison;
- Sources;
- Work;
- Trace.

### Actions

1. Replace the current main-column Chat/Sources/Artifacts/Steps switching with the single summoned canvas.
2. Keep the conversation and composer present while a surface is open.
3. Apply deterministic first-summon behavior:
   - first effectful/multi-step activity → Work;
   - first durable artifact → Output;
   - first citation → Sources;
   - completion → Trace becomes available but does not take focus.
4. Persist available tabs and user-selected focus per thread.
5. Never open two canvases simultaneously.
6. Support open, close, resize, keyboard focus transfer, and return-to-conversation.
7. Render mobile surfaces as full-screen sheets.
8. Sandbox generated HTML with strict capability, network, download, and messaging policies.
9. Treat PDF, browser, file, and HTML as different renderers under the same artifact/surface contract.

### Exit gate

- A conversation can naturally open a PDF, browser, file, table, or generated page without changing routes or becoming an IDE.
- Simple conversations still show no canvas.

## 13. Phase 8 — Ask/Do and long-running agent work

### Ask mode

- Read-only reasoning and retrieval.
- May create non-effectful outputs.
- Sources and Output may be summoned.
- Full read-only message actions remain available.

### Do mode

- Requires an autonomy budget before execution:
  - goal;
  - planned steps;
  - expected duration;
  - allowed tools/surfaces;
  - external systems touched;
  - action classes that require a pause;
  - cost or usage guardrail where relevant.
- Approval is bound to the exact plan version.
- Human edits are echoed back into model context as user-edited instructions.

### Actions

1. Add the visible Ask/Do control.
2. Build the Work surface around plans, todos, steps, checkpoints, approvals, and agent/browser activity.
3. Move approvals into a durable addressable queue.
4. Implement typed pauses: Blocked, Approval, and Ambiguous.
5. Implement accept/deny with guidance and scoped “do not ask again” policies.
6. Support three-way steering:
   - queue guidance while work continues;
   - soft interrupt when in-flight effects are cancellable;
   - hard stop with cancellation receipts.
7. Add live status and attention chips to the thread rail. **Implemented for
   the server-owned latest-run projection:** queued/running and
   approval/paused/failed states are shown as quiet hints; completed runs stay
   visually calm. Full live updates still depend on the thread list being
   refreshed by the caller.
8. Make deep research leave-and-return safe.
9. Generate a complete Trace after the run, without taking focus automatically.

### Exit gate

- Users can leave and return to running work safely.
- The UI always explains what the agent is doing, what it may change, and why it paused.
- Every effect and cancellation is auditable.

## 14. Phase 9 — introduce A2A delegation

### Boundary rule

A2A remains a server-side orchestration concern. The browser sees normalized Verevon delegation events, never remote agent credentials or raw private state.

**Current audit finding (2026-08-30):** the repository contains no verified A2A
agent-card, discovery, task-send, or task-status endpoint. The implementation
must therefore begin with a backend A2A contract spike; the chat UI must not
invent a remote-agent picker or pretend that existing Space agents are A2A
peers. Production delegation stays behind the allowlist and policy gates below.

### Actions

1. Establish a trusted agent registry based on validated Agent Cards.
2. Define capability, modality, authentication, residency, retention, and tenant-allowlist policy.
3. Map A2A tasks, messages, artifacts, status updates, and failures into Verevon UI Event v1.
4. Represent delegation as nested Work activity rather than separate chat personas by default.
5. Preserve parent/child correlation, cancellation, deadlines, and receipts.
6. Validate every returned artifact before exposing it to a renderer.
7. Prevent remote agents from inheriting broader permissions than the initiating user and approved run budget.
8. Add timeout, retry, partial-result, and degraded-agent behavior.
9. Test cross-agent ZDR, erasure, audit, and data-residency propagation.

### Exit gate

- A remote or specialised agent can complete a delegated task while Verevon remains the single coherent user-facing agent.
- Delegation never expands authority silently.

## 15. Phase 10 — Trace, observability, and evaluation

### User-facing Trace

- plan versions and approvals;
- tool and agent activity summaries;
- sources and artifacts;
- effect receipts;
- cancellations and retries;
- model/effort disclosure where permitted;
- context and memory usage summaries;
- cost/latency when product policy says they are useful.

### Operator observability

- event ingest and reducer failures;
- stream latency, reconnects, cursor gaps, duplicates;
- tool and agent failure rates;
- approval dwell time;
- artifact validation and sandbox violations;
- dropped stale projections;
- token/cost/cache telemetry;
- tenant-policy denials;
- frontend long tasks and frame drops.

### Evaluation suites

1. Simple Ask with no panel.
2. Grounded Ask with Sources.
3. PDF upload and preview.
4. Browser session with screenshots and return-after-navigation.
5. Generated HTML preview under sandbox restrictions.
6. Effectful Do run with plan approval and receipt.
7. Mid-run steering, soft interrupt, and hard stop.
8. Reload and resume after every event family.
9. Multiple concurrent threads and browser tabs.
10. A2A delegation with partial failure and cancellation.
11. ZDR, tenant isolation, erasure, and denied-source cases.
12. Keyboard-only, screen reader, zoom, reduced motion, and mobile flows.

### Exit gate

- Product and operator traces agree on run outcomes.
- Critical paths meet reliability, security, accessibility, and performance budgets.

## 16. Phase 11 — migration and staged release

### Migration strategy

1. Keep current routes and SSE endpoints operating while adapters are introduced.
2. Put the new runtime and workspace behind tenant/user feature flags.
3. Render current events through the new reducer in shadow mode and compare state.
4. Enable the new primitives without the workspace canvas.
5. Enable Sources and Output surfaces.
6. Enable Work, Ask/Do, and durable runs.
7. Enable Trace.
8. Enable A2A only for allowlisted agents and tenants.
9. Remove legacy tabs, local-only durable work state, dead flags, and duplicate renderers after parity gates pass.

### Rollout stages

- Internal team and synthetic tenants.
- Design partners with read-only Ask workflows.
- Design partners with controlled Do workflows.
- Tenant-admin opt-in.
- Default-on with rollback flag.
- Legacy removal after the observation window.

### Release gate

- No P0/P1 correctness or security defects.
- Replay parity passes.
- Foreign-origin and effectful-turn invariants pass.
- Accessibility target passes.
- Performance budgets pass on representative long threads.
- Tenant administrators can govern grounding, web access, models/effort, actions, and A2A agents.
- Rollback is tested and does not lose durable work.

## 17. Recommended dependency order

```text
Product contract
  → ownership/effect safety
    → canonical event model
      → durable replay
        → Solid primitives
          → familiar base chat
            → grounding and sources
              → contextual canvas
                → Ask/Do and durable work
                  → A2A delegation
                    → hardening and rollout
```

Parallel work is safe only where contracts are already frozen:

- Visual exploration may run alongside Phase 1.
- Solid primitive prototypes may run alongside event-schema work using golden fixtures.
- Viewer development may run alongside base chat after the surface contract is frozen.
- A2A discovery may run as a backend spike, but production delegation waits for durable runs, receipts, and policy propagation.

## 18. Definition of finished

The transformation is complete when:

1. Verevon opens as a familiar, calm chat experience.
2. Sources appear only when evidence exists.
3. Work appears only when the agent begins durable multi-step activity.
4. Output opens the correct native viewer for PDFs, files, tables, HTML, and other artifacts.
5. Browser sessions are persistent, observable, and safe to leave and return to.
6. Trace explains the completed work without exposing raw hidden reasoning.
7. Ask/Do makes read-only versus effectful intent explicit.
8. Approvals, effects, and cancellations are durable and auditable.
9. Streams resume without losing or duplicating any event family.
10. Specialised/A2A agents can collaborate while Verevon remains the single user-facing agent.
11. Solid’s fine-grained rendering meets performance targets under heavy streaming.
12. The experience works with keyboard, screen reader, zoom, reduced motion, and mobile layouts.
13. Tenant policy governs data, grounding, models, tools, agents, retention, and external effects.
14. Legacy duplicate chat state and dead integration paths have been removed.

## 19. Decisions to revisit as the system grows

- Whether Verevon UI Event should become fully wire-compatible with AG-UI or remain an extended profile.
- Whether Trace needs distinct audit and shareable representations.
- Whether artifact revisions need collaborative editing or remain agent-generated snapshots.
- Whether advanced users may pin a contextual canvas open across turns.
- Whether certain trusted A2A agents can receive standing scoped grants.
- When thread virtualization becomes necessary based on measured transcript size and update frequency.

## 20. Container build verification (2026-08-30)

The frontend image is now buildable through its real Docker path. The
frontend keeps `@quarry/client` as a local generated SDK owned by the
Ingestion Plane, so a frontend-only context cannot resolve it by itself. The
Dockerfile now consumes that sibling SDK through a BuildKit named context;
Compose and direct-build instructions both declare the same context.

Verified commands:

```text
docker build --target build --build-context quarry-client="../../Ingestion Plane/Quarry-v2/sdks/typescript" -t verevonv3-audit-build:local .
docker build --target production --build-context quarry-client="../../Ingestion Plane/Quarry-v2/sdks/typescript" -t verevonv3-audit:local .
```

Both builds passed `pnpm typecheck` and `vite build` (Vite 8.2.2,
TypeScript 7.0.2, 927 modules). The production image was launched in a
disposable container and served `/` with HTTP 200. Its `/health` route is a
gateway proxy by design, so it requires the Compose `inter-plane-bus` network
and the `verevon-gateway-rs` service; it is not expected to answer in an
isolated image-only probe. The disposable container was stopped and removed.

The remaining release gate is therefore runtime composition, not frontend
image packaging: run the production override with real Control/Application/
Model/Data/Ingestion credentials and verify the authenticated gateway path,
SSE replay, browser hand-off, and tenant/ZDR policy propagation end to end.

A Compose build attempt with the repository `.env` was intentionally not
promoted to a stack launch: Compose correctly failed closed because the local
environment does not provide the required gateway/control-plane values
(`INTERNAL_API_KEY`, the core service tokens, and the GDPR NATS password).
No placeholder credentials were introduced to make that gate appear green.

The Verevon v3 CI workflow now carries a separate `docker-image` job. It
regenerates/builds the Quarry SDK and runs the production image build with the
same named context, so a clean checkout catches this cross-plane packaging
failure before release.

The canonical frontend Compose build was also verified with the owner env
files layered by the launcher. Both the default `dev` target and the
production override (`target: production`, with non-secret image metadata
provided) completed successfully without starting or mutating the service
stack.
