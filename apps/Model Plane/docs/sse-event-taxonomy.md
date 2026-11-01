# SSE chat-event taxonomy

<!-- GENERATED FILE — do not edit by hand.
     Regenerate: apps/Model Plane/scripts/gen-sse-event-taxonomy.py
     Verified in CI by .github/workflows/sse-taxonomy.yml -->

The events `model-gateway` can emit on a chat stream, derived from the
`ChatEvent` enum in `model-gateway/src/sse_events.rs`.

`Family` is the opt-in feature family: a client receives a rich event only
if it listed that family in `features[]`. Events with **no** family are
control events and always emit — that is what keeps the plain `chat`
profile working when a client asks for no features at all.

`SPA` is whether `verevonv3`'s stream client has a case for the event. A
missing case means the event is parsed and dropped, so this column is
asserted, not just reported.

| Event | Family | Variant | SPA |
|---|---|---|---|
| `artifact` | `artifacts` | `ChatEvent::Artifact` | ✅ |
| `attachment` | `artifacts` | `ChatEvent::Attachment` | ✅ |
| `citation` | `citations` | `ChatEvent::Citation` | ✅ |
| `compaction` | — (control) | `ChatEvent::Compaction` | ✅ |
| `context_change` | — (control) | `ChatEvent::ContextChange` | ✅ |
| `error` | — (control) | `ChatEvent::Error` | ✅ |
| `error` | — (control) | `ChatEvent::SubscriptionError` | ✅ |
| `error` | — (control) | `ChatEvent::ZdrError` | ✅ |
| `follow_ups` | — (control) | `ChatEvent::FollowUps` | ✅ |
| `grounding` | `citations` | `ChatEvent::Grounding` | ✅ |
| `memory_recall` | `memory` | `ChatEvent::MemoryRecall` | ✅ |
| `plan_proposed` | — (control) | `ChatEvent::PlanProposed` | ✅ |
| `queued_input` | — (control) | `ChatEvent::QueuedInput` | ✅ |
| `queued_input` | — (control) | `ChatEvent::QueuedInputReceipted` | ✅ |
| `reasoning_delta` | `reasoning` | `ChatEvent::ReasoningDelta` | ✅ |
| `result_receipt` | `usage` | `ChatEvent::ResultReceipt` | ✅ |
| `retrieval_notice` | — (control) | `ChatEvent::RetrievalNotice` | ✅ |
| `retry_scheduled` | `steps` | `ChatEvent::RetryScheduled` | ✅ |
| `run_parked` | — (control) | `ChatEvent::RunParked` | ✅ |
| `source_scope_applied` | `citations` | `ChatEvent::SourceScopeApplied` | ✅ |
| `step_update` | `steps` | `ChatEvent::StepUpdate` | ✅ |
| `stopped` | — (control) | `ChatEvent::Stopped` | ✅ |
| `title` | — (control) | `ChatEvent::Title` | ✅ |
| `tool_call` | `tools` | `ChatEvent::ToolCall` | ✅ |
| `tool_result` | `tools` | `ChatEvent::ToolResult` | ✅ |
| `turn_timing` | `usage` | `ChatEvent::TurnTiming` | ✅ |
| `usage` | `usage` | `ChatEvent::Usage` | ✅ |
| `usage_progress` | `usage` | `ChatEvent::UsageProgress` | ✅ |
| `verification` | `citations` | `ChatEvent::Verification` | ✅ |

## Handled by the SPA but not defined by `ChatEvent`

| Event | Why |
|---|---|
| `chunk` | transport: assistant text delta |
| `citations` | legacy alias retained for older gateway builds |
| `connected` | transport: stream opened, emitted by the SSE layer |
| `done` | transport: terminal success frame |
| `search_results` | legacy alias retained for older gateway builds |

Anything the SPA does not recognise reaches `onUnknownEvent`, which logs
rather than discarding silently — the runtime counterpart to this file.

## Other producers

Frames model-gateway writes that `ChatEvent` does not name, read from source: a
literal `.event("...")` in production code (test modules are skipped), and the
`OrchestrationEvent` oneof, whose wire name is its proto field name.

| Event | Why |
|---|---|
| `awaiting_approval` | agentic run paused for approval at the run boundary, before a derived `step_update` exists |
| `chunk` | transport: assistant text delta |
| `connected` | transport: stream opened, emitted by the SSE layer |
| `done` | transport: terminal success frame |
| `error` | shared with `ChatEvent::Error`; the run-events stream writes it directly when it cannot authorise or open the orchestration stream |

The run-events stream (`/v1/runs/:run_id/events`) carries the orchestration
events. Tags are the proto field numbers. `run-console-client.ts` reads them;
this gate checks only the producer side. A member with no arm is declared on
the wire but not written yet.

| Event | Tag | Written |
|---|---|---|
| `plan_transitioned` | 10 | ✅ |
| `todo_transitioned` | 11 | ✅ |
| `approval_state_changed` | 12 | ✅ |
| `subagent_attached` | 13 | ✅ |
| `subagent_stopped` | 14 | ✅ |
| `run_paused_for_approval` | 15 | ✅ |
| `run_resumed_after_approval` | 16 | ✅ |
| `browser_action_dispatched` | 17 | ✅ |
| `browser_observation_received` | 18 | ✅ |
| `browser_run_paused` | 19 | ✅ |
| `browser_run_resumed` | 20 | ✅ |
| `browser_action_approval_required` | 21 | ✅ |
| `browser_action_decided` | 22 | ✅ |
| `approval_continuation_verified` | 23 | ✅ |
| `run_parked` | 24 | ✅ |
| `run_resumed` | 25 | ✅ |
| `tool_retry_scheduled` | 26 | ✅ |
| `question_asked` | 27 | not yet |
| `question_answered` | 28 | not yet |
| `run_usage_progress` | 29 | not yet |
