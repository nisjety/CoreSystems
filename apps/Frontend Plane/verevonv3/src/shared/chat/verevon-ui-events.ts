/** Stable Verevon-owned UI events projected from the backend SSE stream. */
export type VerevonPauseKind = 'blocked' | 'approval' | 'ambiguous'

export type VerevonUiEvent =
  | { type: 'run.connected'; at: string; requestId?: string; threadId?: string; runId?: string; model?: string }
  | { type: 'run.completed'; at: string; runId?: string; outcome?: string }
  | { type: 'run.paused'; at: string; runId?: string; interrupts?: unknown[]; pauseKind?: VerevonPauseKind; detail?: string }
  | { type: 'run.resumed'; at: string; runId?: string; approvalId?: string }
  | { type: 'run.cancelled'; at: string; runId?: string; receiptId?: string; reason?: string }
  | { type: 'message.delta'; at: string; delta: string; runId?: string; messageId?: string }
  | { type: 'message.started'; at: string; messageId?: string; role?: string; runId?: string }
  | { type: 'message.done'; at: string; requestId?: string; messageId?: string; runId?: string; modelUsed?: string; outputTokens?: number; stopReason?: string }
  | { type: 'run.stopped'; at: string; requestId?: string; reason?: string }
  | { type: 'run.error'; at: string; code: string; message: string; retryable?: boolean; runId?: string; requestId?: string }
  | { type: 'reasoning.delta'; at: string; delta: string; runId?: string }
  | { type: 'tool.call'; at: string; id?: string; name?: string; args?: unknown; runId?: string }
  | { type: 'tool.result'; at: string; id?: string; status?: string; output?: string; error?: string; runId?: string }
  | { type: 'artifact.updated'; at: string; id?: string; kind?: string; title?: string; version?: number }
  | { type: 'attachment.created'; at: string; id?: string; name?: string; mime?: string; typeName?: string; url?: string; size?: number }
  | { type: 'citation.added'; at: string; id?: string; title?: string; url?: string; snippet?: string; claimId?: string; sourceGroupId?: string; start?: number; end?: number }
  | { type: 'grounding.updated'; at: string; value: unknown }
  | { type: 'step.updated'; at: string; id?: string; title?: string; detail?: string; status?: string }
  | { type: 'state.snapshot'; at: string; value: unknown }
  | { type: 'state.delta'; at: string; patch: unknown[] }
  | { type: 'activity.updated'; at: string; id?: string; activityType?: string; value?: unknown; patch?: unknown[] }
  | { type: 'delegation.updated'; at: string; runId?: string; childRunId?: string; status?: string; detail?: string }
  | { type: 'approval.requested'; at: string; runId?: string; approvalId?: string; kind?: string; detail?: string }
  | { type: 'approval.decided'; at: string; runId?: string; approvalId?: string; decision?: string; detail?: string }
  | { type: 'receipt.verified'; at: string; runId?: string; receiptId?: string; status?: string; detail?: string }
  /** Envelope-only event recovered from Session Core's durable thread log. */
  | { type: 'trace.replayed'; at: string; eventId: string; eventType: string; producer?: string; resourceRef?: string; schemaVersion?: number }
  | { type: 'usage.recorded'; at: string; inputTokens?: number; outputTokens?: number; latencyMs?: number; costUsd?: number; confidence?: number; cacheReadTokens?: number; cacheWriteTokens?: number }
  | { type: 'memory.recalled'; at: string; count: number; latencyMs?: number; memories: unknown[] }
  | { type: 'input.queued'; at: string; messages: string[] }
  | { type: 'thread.titled'; at: string; title: string }
  | { type: 'followups.suggested'; at: string; suggestions: string[] }
  | { type: 'unknown'; at: string; name: string; payload: Record<string, unknown> }

const stringValue = (value: unknown): string | undefined => typeof value === 'string' ? value : undefined
const numberValue = (value: unknown): number | undefined => typeof value === 'number' && Number.isFinite(value) ? value : undefined
const stringArray = (value: unknown): string[] => Array.isArray(value) ? value.filter((entry): entry is string => typeof entry === 'string') : []
const recordValue = (value: unknown): Record<string, unknown> => (
  value && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : {}
)

function pauseKindFrom(value: unknown): VerevonPauseKind | undefined {
  if (typeof value !== 'string') return undefined
  const normalized = value.trim().toLowerCase().replaceAll('-', '_').replaceAll(' ', '_')
  if (normalized.includes('approval') || normalized.includes('consent') || normalized.includes('permission')) return 'approval'
  if (normalized.includes('ambiguous') || normalized.includes('uncertain') || normalized.includes('clarif')) return 'ambiguous'
  if (normalized.includes('blocked') || normalized.includes('policy') || normalized.includes('denied')) return 'blocked'
  return undefined
}

function pauseKindFromPayload(payload: Record<string, unknown>): VerevonPauseKind | undefined {
  return pauseKindFrom(
    payload.pause_kind
      ?? payload.pauseKind
      ?? payload.reason
      ?? payload.status
      ?? payload.kind,
  )
}

/** AG-UI event names are uppercase by protocol convention. Keep this list
 * explicit so a future custom event cannot accidentally be treated as a
 * standard event and mutate the chat through the compatibility path. */
const AG_UI_EVENT_NAMES = new Set([
  'RUN_STARTED', 'RUN_FINISHED', 'RUN_ERROR',
  'STEP_STARTED', 'STEP_FINISHED',
  'TEXT_MESSAGE_START', 'TEXT_MESSAGE_CONTENT', 'TEXT_MESSAGE_END', 'TEXT_MESSAGE_CHUNK',
  'TOOL_CALL_START', 'TOOL_CALL_ARGS', 'TOOL_CALL_END', 'TOOL_CALL_RESULT', 'TOOL_CALL_CHUNK',
  'STATE_SNAPSHOT', 'STATE_DELTA', 'MESSAGES_SNAPSHOT',
  'ACTIVITY_SNAPSHOT', 'ACTIVITY_DELTA',
  'REASONING_START', 'REASONING_MESSAGE_START', 'REASONING_MESSAGE_CONTENT', 'REASONING_MESSAGE_END', 'REASONING_END', 'REASONING_MESSAGE_CHUNK',
  'SUBAGENT_STARTED', 'SUBAGENT_FINISHED', 'SUBAGENT_ERROR',
  'RAW', 'CUSTOM',
])

/** CUSTOM names the gateway is allowed to project into Verevon semantics.
 * Native lifecycle names such as `chunk`, `done`, or `error` are intentionally
 * absent: a custom envelope must never impersonate a terminal or message
 * event. */
const PROJECTABLE_CUSTOM_NAMES = new Set([
  // Canonical names emitted by the reverse AG-UI serializer. These remain
  // explicit projections, never executable commands.
  'artifact.updated',
  'attachment.created',
  'citation.added',
  'grounding.updated',
  'step.updated',
  'delegation.updated',
  'approval.requested',
  'approval.decided',
  'receipt.verified',
  'run.paused',
  'run.resumed',
  'run.cancelled',
  'run.stopped',
  'usage.recorded',
  'memory.recalled',
  'input.queued',
  'thread.titled',
  'followups.suggested',
  'trace.replayed',
  'artifact',
  'attachment',
  'reasoning_delta',
  'citation',
  'grounding',
  'step_update',
  'tool_result',
  'usage',
  'awaiting_approval',
  'run_paused_for_approval',
  'run_paused',
  'run_resumed_after_approval',
  'approval_state_changed',
  'browser_action_approval_required',
  'browser_action_decided',
  'approval_continuation_verified',
  'browser_run_paused',
  'browser_run_resumed',
])

const CANONICAL_CUSTOM_EVENT_NAMES = new Set([
  'artifact.updated',
  'attachment.created',
  'citation.added',
  'grounding.updated',
  'step.updated',
  'delegation.updated',
  'approval.requested',
  'approval.decided',
  'receipt.verified',
  'run.paused',
  'run.resumed',
  'run.cancelled',
  'run.stopped',
  'usage.recorded',
  'memory.recalled',
  'input.queued',
  'thread.titled',
  'followups.suggested',
  'trace.replayed',
])

export function isAgUiEventName(name: string | undefined): boolean {
  return AG_UI_EVENT_NAMES.has((name ?? '').trim().toUpperCase())
}

function agUiMetadata(payload: Record<string, unknown>): Record<string, unknown> {
  return recordValue(payload.metadata)
}

function agUiField(payload: Record<string, unknown>, ...names: string[]): unknown {
  const metadata = agUiMetadata(payload)
  for (const name of names) {
    if (payload[name] !== undefined) return payload[name]
    if (metadata[name] !== undefined) return metadata[name]
  }
  return undefined
}

export function toVerevonUiEvent(name: string, payload: Record<string, unknown>, at = new Date().toISOString()): VerevonUiEvent {
  const protocolName = isAgUiEventName(name) ? name.trim().toUpperCase() : name.trim()
  switch (protocolName) {
    case 'connected': return { type: 'run.connected', at, requestId: stringValue(payload.request_id), threadId: stringValue(payload.thread_id), runId: stringValue(payload.run_id), model: stringValue(payload.model_used) ?? stringValue(payload.model) }
    case 'RUN_STARTED': return { type: 'run.connected', at, threadId: stringValue(payload.threadId) ?? stringValue(payload.thread_id), runId: stringValue(payload.runId) ?? stringValue(payload.run_id), requestId: stringValue(agUiField(payload, 'requestId', 'request_id')), model: stringValue(agUiField(payload, 'model', 'modelUsed', 'model_used')) }
    case 'RUN_FINISHED': {
      const outcome = recordValue(payload.outcome)
      const outcomeType = stringValue(outcome.type)?.toLowerCase()
      const outcomeStatus = stringValue(outcome.status)?.toLowerCase()
      // AG-UI represents an interrupted run with RUN_FINISHED plus a
      // discriminated `{ type: "interrupt", interrupts: [...] }` outcome.
      // It is not a successful terminal answer: projecting it as completed
      // would make the Work rail disappear and strand approval controls.
      if (outcomeType === 'interrupt') {
        return {
          type: 'run.paused',
          at,
          runId: stringValue(payload.runId) ?? stringValue(payload.run_id),
          interrupts: Array.isArray(outcome.interrupts) ? outcome.interrupts : undefined,
          detail: stringValue(outcome.reason) ?? stringValue(payload.message),
        }
      }
      // The gateway preserves a native hard stop as an additive AG-UI status
      // while still closing the standard lifecycle. Do not turn that receipt
      // into a successful completion in the Verevon projection.
      if (outcomeStatus === 'stopped' || outcomeStatus === 'cancelled') {
        return {
          type: 'run.stopped',
          at,
          requestId: stringValue(payload.requestId) ?? stringValue(payload.request_id),
          reason: stringValue(payload.reason) ?? stringValue(payload.message),
        }
      }
      return {
        type: 'run.completed',
        at,
        runId: stringValue(payload.runId) ?? stringValue(payload.run_id),
        outcome: outcomeStatus ?? outcomeType ?? stringValue(payload.outcome),
      }
    }
    case 'RUN_CANCELLED':
      return {
        type: 'run.cancelled',
        at,
        runId: stringValue(payload.run_id) ?? stringValue(payload.runId),
        receiptId: stringValue(payload.receipt_id) ?? stringValue(payload.receiptId) ?? stringValue(payload.event_id),
        reason: stringValue(payload.reason),
      }
    case 'run_paused_for_approval':
      return {
        type: 'run.paused',
        at,
        runId: stringValue(payload.run_id) ?? stringValue(payload.runId),
        interrupts: Array.isArray(payload.interrupts)
          ? payload.interrupts
          : Array.isArray(payload.approvals) ? payload.approvals : undefined,
        pauseKind: 'approval',
        detail: stringValue(payload.reason) ?? stringValue(payload.detail),
      }
    case 'awaiting_approval':
      return {
        type: 'run.paused',
        at,
        runId: stringValue(payload.run_id) ?? stringValue(payload.runId),
        pauseKind: 'approval',
        detail: stringValue(payload.reason) ?? stringValue(payload.detail),
      }
    case 'run_resumed_after_approval':
      return {
        type: 'run.resumed',
        at,
        runId: stringValue(payload.run_id) ?? stringValue(payload.runId),
        approvalId: stringValue(payload.approval_id) ?? stringValue(payload.approvalId),
      }
    case 'approval_state_changed': {
      const status = stringValue(payload.to) ?? stringValue(payload.state) ?? stringValue(payload.status)
      const approval = {
        at,
        runId: stringValue(payload.run_id) ?? stringValue(payload.runId),
        approvalId: stringValue(payload.approval_id) ?? stringValue(payload.approvalId),
        kind: stringValue(payload.approval_kind) ?? stringValue(payload.approvalKind) ?? stringValue(payload.kind),
        detail: status,
      }
      return status && ['pending', 'requested', 'waiting', 'awaiting_approval'].includes(status.toLowerCase())
        ? { type: 'approval.requested', ...approval }
        : { type: 'approval.decided', ...approval, decision: status }
    }
    case 'browser_action_approval_required':
      return {
        type: 'approval.requested',
        at,
        runId: stringValue(payload.run_id) ?? stringValue(payload.runId),
        approvalId: stringValue(payload.approval_id) ?? stringValue(payload.approvalId),
        kind: stringValue(payload.risk_category) ?? stringValue(payload.riskCategory),
        detail: [stringValue(payload.action_type) ?? stringValue(payload.actionType), stringValue(payload.reason)]
          .filter(Boolean)
          .join(' · ') || undefined,
      }
    case 'browser_action_decided':
      return {
        type: 'approval.decided',
        at,
        runId: stringValue(payload.run_id) ?? stringValue(payload.runId),
        approvalId: stringValue(payload.approval_id) ?? stringValue(payload.approvalId),
        decision: stringValue(payload.decision),
        detail: stringValue(payload.action_type) ?? stringValue(payload.actionType),
      }
    case 'browser_run_paused':
      return {
        type: 'run.paused',
        at,
        runId: stringValue(payload.run_id) ?? stringValue(payload.runId),
        pauseKind: pauseKindFromPayload(payload),
        detail: stringValue(payload.reason) ?? stringValue(payload.detail),
      }
    case 'run_paused':
      return {
        type: 'run.paused',
        at,
        runId: stringValue(payload.run_id) ?? stringValue(payload.runId),
        interrupts: Array.isArray(payload.interrupts) ? payload.interrupts : undefined,
        pauseKind: pauseKindFromPayload(payload),
        detail: stringValue(payload.reason) ?? stringValue(payload.detail),
      }
    case 'browser_run_resumed':
      return {
        type: 'run.resumed',
        at,
        runId: stringValue(payload.run_id) ?? stringValue(payload.runId),
      }
    case 'approval_continuation_verified': {
      const verification = recordValue(payload.verification)
      return {
        type: 'receipt.verified',
        at,
        runId: stringValue(payload.run_id) ?? stringValue(payload.runId),
        receiptId: stringValue(payload.receipt_id) ?? stringValue(payload.receiptId),
        status: stringValue(payload.verification_status)
          ?? stringValue(payload.verificationStatus)
          ?? stringValue(verification.status),
        detail: stringValue(payload.verification_reason)
          ?? stringValue(payload.verificationReason)
          ?? stringValue(verification.reason),
      }
    }
    case 'RUN_ERROR': return { type: 'run.error', at, code: stringValue(payload.code) ?? 'run_error', message: stringValue(payload.message) ?? 'Agent run failed', retryable: payload.retryable === true, runId: stringValue(payload.runId) ?? stringValue(payload.run_id), requestId: stringValue(agUiField(payload, 'requestId', 'request_id')) }
    case 'chunk': return { type: 'message.delta', at, delta: stringValue(payload.delta) ?? stringValue(payload.content) ?? '' }
    case 'TEXT_MESSAGE_START': return { type: 'message.started', at, messageId: stringValue(payload.messageId) ?? stringValue(payload.message_id), role: stringValue(payload.role), runId: stringValue(payload.runId) ?? stringValue(payload.run_id) }
    case 'TEXT_MESSAGE_CONTENT':
    case 'TEXT_MESSAGE_CHUNK': return { type: 'message.delta', at, delta: stringValue(payload.delta) ?? stringValue(payload.content) ?? '', runId: stringValue(payload.runId) ?? stringValue(payload.run_id), messageId: stringValue(payload.messageId) ?? stringValue(payload.message_id) }
    case 'TEXT_MESSAGE_END': return { type: 'message.done', at, requestId: stringValue(agUiField(payload, 'requestId', 'request_id')), messageId: stringValue(payload.messageId) ?? stringValue(payload.message_id), runId: stringValue(payload.runId) ?? stringValue(payload.run_id), stopReason: stringValue(agUiField(payload, 'finishReason', 'finish_reason')) }
    case 'done': return { type: 'message.done', at, requestId: stringValue(payload.request_id), modelUsed: stringValue(payload.model_used) ?? stringValue(payload.modelUsed), outputTokens: numberValue(payload.output_tokens) ?? numberValue(payload.outputTokens), stopReason: stringValue(payload.stop_reason) ?? stringValue(payload.stopReason) }
    case 'stopped': return { type: 'run.stopped', at, requestId: stringValue(payload.request_id), reason: stringValue(payload.reason) }
    case 'error': return { type: 'run.error', at, code: stringValue(payload.code) ?? 'stream_error', message: stringValue(payload.message) ?? 'Stream interrupted', retryable: payload.retryable === true, runId: stringValue(payload.run_id) ?? stringValue(payload.runId), requestId: stringValue(payload.request_id) ?? stringValue(payload.requestId) }
    case 'reasoning_delta': return { type: 'reasoning.delta', at, delta: stringValue(payload.delta) ?? '', runId: stringValue(payload.run_id) ?? stringValue(payload.runId) }
    case 'tool_call': return { type: 'tool.call', at, id: stringValue(payload.id), name: stringValue(payload.name), args: payload.args, runId: stringValue(payload.run_id) ?? stringValue(payload.runId) }
    case 'TOOL_CALL_START': return { type: 'tool.call', at, id: stringValue(payload.toolCallId) ?? stringValue(payload.tool_call_id), name: stringValue(payload.toolCallName) ?? stringValue(payload.tool_call_name), runId: stringValue(payload.runId) ?? stringValue(payload.run_id) }
    case 'TOOL_CALL_ARGS': return { type: 'tool.call', at, id: stringValue(payload.toolCallId) ?? stringValue(payload.tool_call_id), args: stringValue(payload.delta) ?? payload.delta, runId: stringValue(payload.runId) ?? stringValue(payload.run_id) }
    case 'tool_result': return { type: 'tool.result', at, id: stringValue(payload.id), status: stringValue(payload.status), output: stringValue(payload.output), error: stringValue(payload.error), runId: stringValue(payload.run_id) ?? stringValue(payload.runId) }
    case 'TOOL_CALL_RESULT': return { type: 'tool.result', at, id: stringValue(payload.toolCallId) ?? stringValue(payload.tool_call_id), status: 'completed', output: stringValue(payload.content) ?? stringValue(payload.output) ?? stringValue(payload.result), runId: stringValue(payload.runId) ?? stringValue(payload.run_id) }
    case 'artifact': return { type: 'artifact.updated', at, id: stringValue(payload.id), kind: stringValue(payload.kind), title: stringValue(payload.title), version: numberValue(payload.version) }
    case 'attachment': return { type: 'attachment.created', at, id: stringValue(payload.id), name: stringValue(payload.name), mime: stringValue(payload.mime), typeName: stringValue(payload.type), url: stringValue(payload.url), size: numberValue(payload.size) }
    case 'citation': return {
      type: 'citation.added',
      at,
      id: stringValue(payload.id),
      title: stringValue(payload.title),
      url: stringValue(payload.url),
      snippet: stringValue(payload.snippet),
      claimId: stringValue(payload.claim_id) ?? stringValue(payload.claimId),
      sourceGroupId: stringValue(payload.source_group_id) ?? stringValue(payload.sourceGroupId),
      start: numberValue(payload.start) ?? numberValue(payload.start_offset) ?? numberValue(payload.startOffset),
      end: numberValue(payload.end) ?? numberValue(payload.end_offset) ?? numberValue(payload.endOffset),
    }
    case 'grounding': return { type: 'grounding.updated', at, value: payload.value }
    case 'step':
    case 'step_update': return { type: 'step.updated', at, id: stringValue(payload.id), title: stringValue(payload.title), detail: stringValue(payload.detail), status: stringValue(payload.status) }
    case 'STEP_STARTED': return { type: 'step.updated', at, id: stringValue(payload.stepName), title: stringValue(payload.stepName), status: 'started' }
    case 'STEP_FINISHED': return { type: 'step.updated', at, id: stringValue(payload.stepName), title: stringValue(payload.stepName), status: 'finished' }
    case 'STATE_SNAPSHOT': return { type: 'state.snapshot', at, value: payload.snapshot }
    case 'STATE_DELTA': return { type: 'state.delta', at, patch: Array.isArray(payload.delta) ? payload.delta : [] }
    case 'ACTIVITY_SNAPSHOT': return { type: 'activity.updated', at, id: stringValue(payload.messageId), activityType: stringValue(payload.activityType), value: payload.content }
    case 'ACTIVITY_DELTA': return { type: 'activity.updated', at, id: stringValue(payload.messageId), activityType: stringValue(payload.activityType), patch: Array.isArray(payload.patch) ? payload.patch : [] }
    case 'SUBAGENT_STARTED': return { type: 'delegation.updated', at, runId: stringValue(payload.parentRunId) ?? stringValue(payload.parent_run_id), childRunId: stringValue(payload.runId) ?? stringValue(payload.run_id), status: 'started', detail: stringValue(payload.agentId) ?? stringValue(payload.agent_id) }
    case 'SUBAGENT_FINISHED': return { type: 'delegation.updated', at, runId: stringValue(payload.parentRunId) ?? stringValue(payload.parent_run_id), childRunId: stringValue(payload.runId) ?? stringValue(payload.run_id), status: 'finished' }
    case 'SUBAGENT_ERROR': return { type: 'delegation.updated', at, runId: stringValue(payload.parentRunId) ?? stringValue(payload.parent_run_id), childRunId: stringValue(payload.runId) ?? stringValue(payload.run_id), status: 'error', detail: stringValue(payload.message) }
    case 'REASONING_MESSAGE_CONTENT':
    case 'REASONING_MESSAGE_CHUNK': return { type: 'reasoning.delta', at, delta: stringValue(payload.delta) ?? stringValue(payload.content) ?? '', runId: stringValue(payload.runId) ?? stringValue(payload.run_id) }
    case 'REASONING_START':
    case 'REASONING_MESSAGE_START': return { type: 'reasoning.delta', at, delta: '' }
    case 'REASONING_MESSAGE_END':
    case 'REASONING_END': return { type: 'reasoning.delta', at, delta: '' }
    case 'usage': return { type: 'usage.recorded', at, inputTokens: numberValue(payload.input_tokens) ?? numberValue(payload.inputTokens), outputTokens: numberValue(payload.output_tokens) ?? numberValue(payload.outputTokens), latencyMs: numberValue(payload.latency_ms) ?? numberValue(payload.latencyMs), costUsd: numberValue(payload.cost_usd) ?? numberValue(payload.costUsd), confidence: numberValue(payload.confidence), cacheReadTokens: numberValue(payload.cache_read_tokens) ?? numberValue(payload.cacheReadTokens), cacheWriteTokens: numberValue(payload.cache_write_tokens) ?? numberValue(payload.cacheWriteTokens) }
    case 'memory_recall': return { type: 'memory.recalled', at, count: numberValue(payload.count) ?? 0, latencyMs: numberValue(payload.latency_ms) ?? numberValue(payload.latencyMs), memories: Array.isArray(payload.memories) ? payload.memories : [] }
    case 'queued_input': return { type: 'input.queued', at, messages: stringArray(payload.messages) }
    case 'title': return { type: 'thread.titled', at, title: stringValue(payload.title) ?? '' }
    case 'follow_ups': return { type: 'followups.suggested', at, suggestions: stringArray(payload.suggestions) }
    case 'CUSTOM': {
      const customName = stringValue(payload.name)
      const customValue = recordValue(payload.value)
      if (
        customName
        && CANONICAL_CUSTOM_EVENT_NAMES.has(customName)
        && stringValue(customValue.type) === customName
      ) {
        // Reverse-serialized canonical events carry their discriminant in the
        // value. Accept only names from the explicit set above; arbitrary
        // CUSTOM payloads remain opaque and cannot mutate the chat.
        return { ...customValue, at: stringValue(customValue.at) ?? at } as VerevonUiEvent
      }
      // Only recurse for the explicit, server-produced CUSTOM envelope. The
      // nested name is still constrained by the cases above; arbitrary custom
      // names remain opaque `unknown` events.
      if (customName && PROJECTABLE_CUSTOM_NAMES.has(customName)) {
        return toVerevonUiEvent(customName, customValue, at)
      }
      return { type: 'unknown', at, name: protocolName, payload }
    }
    default: return { type: 'unknown', at, name: protocolName, payload }
  }
}

export function uiEventLabel(event: VerevonUiEvent): string {
  if (event.type === 'trace.replayed') return event.eventType.replace(/^EVENT_TYPE_/, '').replaceAll('_', ' ')
  if (event.type === 'run.paused' && event.pauseKind) return `run · paused · ${event.pauseKind}`
  return event.type.replace('.', ' · ')
}
