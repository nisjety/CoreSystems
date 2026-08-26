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
| `error` | — (control) | `ChatEvent::Error` | ✅ |
| `follow_ups` | — (control) | `ChatEvent::FollowUps` | ✅ |
| `grounding` | `citations` | `ChatEvent::Grounding` | ✅ |
| `memory_recall` | `memory` | `ChatEvent::MemoryRecall` | ✅ |
| `queued_input` | — (control) | `ChatEvent::QueuedInput` | ✅ |
| `reasoning_delta` | `reasoning` | `ChatEvent::ReasoningDelta` | ✅ |
| `step_update` | `steps` | `ChatEvent::StepUpdate` | ✅ |
| `stopped` | — (control) | `ChatEvent::Stopped` | ✅ |
| `title` | — (control) | `ChatEvent::Title` | ✅ |
| `tool_call` | `tools` | `ChatEvent::ToolCall` | ✅ |
| `tool_result` | `tools` | `ChatEvent::ToolResult` | ✅ |
| `usage` | `usage` | `ChatEvent::Usage` | ✅ |

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
