import {
  buildChatWireBody,
  type ChatInvokeRequest,
  type ChatStreamHandlers,
} from './chat-client'
import { readSseStream, type SseEvent } from './sse'
import { toVerevonUiEvent, type VerevonUiEvent } from '@/shared/chat/verevon-ui-events'

export const AG_UI_EVENT_TYPES = [
  'RUN_STARTED',
  'RUN_FINISHED',
  'RUN_ERROR',
  'STEP_STARTED',
  'STEP_FINISHED',
  'TEXT_MESSAGE_START',
  'TEXT_MESSAGE_CONTENT',
  'TEXT_MESSAGE_END',
  'TEXT_MESSAGE_CHUNK',
  'TOOL_CALL_START',
  'TOOL_CALL_ARGS',
  'TOOL_CALL_END',
  'TOOL_CALL_RESULT',
  'TOOL_CALL_CHUNK',
  'STATE_SNAPSHOT',
  'STATE_DELTA',
  'MESSAGES_SNAPSHOT',
  'ACTIVITY_SNAPSHOT',
  'ACTIVITY_DELTA',
  'REASONING_START',
  'REASONING_MESSAGE_START',
  'REASONING_MESSAGE_CONTENT',
  'REASONING_MESSAGE_END',
  'REASONING_END',
  'SUBAGENT_STARTED',
  'SUBAGENT_FINISHED',
  'SUBAGENT_ERROR',
  'RAW',
  'CUSTOM',
] as const

export type AgUiEventType = (typeof AG_UI_EVENT_TYPES)[number]

export type AgUiEvent = {
  type: AgUiEventType | string
  /** AG-UI base event metadata. Kept optional for legacy native frames. */
  timestamp?: string
  metadata?: Record<string, unknown>
  rawEvent?: unknown
  source?: string
  runId?: string
  parentRunId?: string
  requestId?: string
  threadId?: string
  messageId?: string
  parentMessageId?: string
  role?: string
  /** Text/tool fragments are strings; StateDelta uses JSON Patch arrays. */
  delta?: string | unknown[]
  content?: unknown
  snapshot?: unknown
  patch?: unknown[]
  messages?: unknown[]
  activityType?: string
  replace?: boolean
  event?: unknown
  outcome?: unknown
  input?: unknown
  message?: string
  code?: string
  toolCallId?: string
  toolCallName?: string
  stepName?: string
  agentId?: string
  subagentRunId?: string
  args?: unknown
  name?: string
  value?: unknown
  modelUsed?: string
  outputTokens?: number
}

export type AgUiStreamHandlers = {
  onEvent?: (event: AgUiEvent) => void
  /** Canonical Verevon projection for lifecycle events that have no focused
   * compatibility callback (for example an AG-UI interrupt outcome). */
  onUiEvent?: (event: VerevonUiEvent) => void
  onRunStarted?: (event: AgUiEvent) => void
  onRunFinished?: (event: AgUiEvent) => void
  onRunError?: (event: AgUiEvent) => void
  onTextMessageContent?: (event: AgUiEvent) => void
  onToolCall?: (event: AgUiEvent) => void
  onStep?: (event: AgUiEvent) => void
  onReasoning?: (event: AgUiEvent) => void
  onSubagent?: (event: AgUiEvent) => void
  onCustom?: (event: AgUiEvent) => void
  onFrameId?: (id: string) => void
}

/**
 * Reverse compatibility mapping for public Verevon events.
 *
 * Verevon remains the internal authority, so this is deliberately a pure
 * serializer rather than a second transport or mutation path. Events without
 * a safe AG-UI equivalent are represented as CUSTOM with a namespaced value;
 * an opaque/unknown event returns null and must not be emitted as a command.
 */
export function toAgUiEvent(event: VerevonUiEvent): AgUiEvent | null {
  const base = {
    timestamp: event.at,
    metadata: { verevon: { adapter: 'verevon-ui', protocolVersion: '1' } },
  }
  switch (event.type) {
    case 'run.connected':
      return { ...base, type: 'RUN_STARTED', runId: event.runId, threadId: event.threadId, requestId: event.requestId, input: event.model ? { model: event.model } : undefined }
    case 'run.completed':
      return { ...base, type: 'RUN_FINISHED', runId: event.runId, outcome: { type: 'success', status: event.outcome ?? 'completed' } }
    case 'message.started':
      return { ...base, type: 'TEXT_MESSAGE_START', messageId: event.messageId, role: event.role ?? 'assistant', runId: event.runId }
    case 'message.delta':
      return { ...base, type: 'TEXT_MESSAGE_CHUNK', delta: event.delta, runId: event.runId, messageId: event.messageId }
    case 'message.done':
      return { ...base, type: 'TEXT_MESSAGE_END', messageId: event.messageId, runId: event.runId, requestId: event.requestId, metadata: { ...base.metadata, finishReason: event.stopReason, modelUsed: event.modelUsed, outputTokens: event.outputTokens } }
    case 'run.paused':
      // AG-UI requires a non-empty interrupt list for the interrupt outcome;
      // when Verevon has no authoritative interrupt record, keep the typed
      // pause as an opaque custom event instead of manufacturing one.
      if (event.interrupts && event.interrupts.length > 0) {
        return { ...base, type: 'RUN_FINISHED', runId: event.runId, outcome: { type: 'interrupt', interrupts: event.interrupts } }
      }
      return { ...base, type: 'CUSTOM', name: 'run_paused', value: { run_id: event.runId, pause_kind: event.pauseKind, detail: event.detail } }
    case 'tool.call':
      if (event.args !== undefined) {
        return {
          ...base,
          type: 'TOOL_CALL_CHUNK',
          toolCallId: event.id,
          toolCallName: event.name,
          delta: textValue(event.args),
          runId: event.runId,
        }
      }
      return { ...base, type: 'TOOL_CALL_START', toolCallId: event.id, toolCallName: event.name, runId: event.runId }
    case 'tool.result':
      return { ...base, type: 'TOOL_CALL_RESULT', toolCallId: event.id, content: event.output, runId: event.runId }
    case 'reasoning.delta':
      return { ...base, type: 'REASONING_MESSAGE_CONTENT', delta: event.delta, runId: event.runId }
    case 'state.snapshot':
      return { ...base, type: 'STATE_SNAPSHOT', snapshot: event.value }
    case 'state.delta':
      return { ...base, type: 'STATE_DELTA', delta: event.patch }
    case 'activity.updated':
      return { ...base, type: event.patch ? 'ACTIVITY_DELTA' : 'ACTIVITY_SNAPSHOT', messageId: event.id, activityType: event.activityType, patch: event.patch, content: event.value }
    case 'artifact.updated':
    case 'attachment.created':
    case 'citation.added':
    case 'grounding.updated':
    case 'step.updated':
    case 'delegation.updated':
    case 'approval.requested':
    case 'approval.decided':
    case 'receipt.verified':
    case 'run.resumed':
    case 'run.cancelled':
    case 'run.stopped':
    case 'usage.recorded':
    case 'memory.recalled':
    case 'input.queued':
    case 'thread.titled':
    case 'followups.suggested':
    case 'trace.replayed':
      return { ...base, type: 'CUSTOM', name: event.type, value: event }
    case 'run.error':
      return { ...base, type: 'RUN_ERROR', runId: event.runId, requestId: event.requestId, code: event.code, message: event.message }
    case 'unknown':
      return null
  }
}

function str(value: unknown): string | undefined {
  return typeof value === 'string' ? value : undefined
}

function num(value: unknown): number | undefined {
  return typeof value === 'number' ? value : undefined
}

function textValue(value: unknown): string | undefined {
  if (typeof value === 'string') return value
  if (value == null) return undefined
  try {
    return JSON.stringify(value)
  } catch {
    return String(value)
  }
}

function objectValue(value: unknown): Record<string, unknown> | undefined {
  return value && typeof value === 'object' ? value as Record<string, unknown> : undefined
}

function argumentValue(event: AgUiEvent): unknown {
  if (event.args !== undefined) return event.args
  // AG-UI ToolCallArgs carries a JSON-fragment `delta`. Keep a raw fragment
  // when it is incomplete; parse complete JSON so legacy focused renderers
  // still receive the structured argument value they expect.
  if (typeof event.delta !== 'string') return undefined
  try {
    return JSON.parse(event.delta) as unknown
  } catch {
    return event.delta
  }
}

function createRunId(prefix: string): string {
  if (typeof crypto !== 'undefined' && 'randomUUID' in crypto) {
    return `${prefix}-${crypto.randomUUID()}`
  }
  return `${prefix}-${Date.now()}-${Math.random().toString(36).slice(2, 10)}`
}

function parseParametersJson(parametersJson: unknown): Record<string, unknown> {
  if (typeof parametersJson !== 'string' || !parametersJson.trim()) {
    return { type: 'object', properties: {} }
  }

  try {
    const parsed = JSON.parse(parametersJson)
    return parsed && typeof parsed === 'object'
      ? parsed as Record<string, unknown>
      : { type: 'object', properties: {} }
  } catch {
    return { type: 'object', properties: {} }
  }
}

export function buildAgentRunInputBody(request: ChatInvokeRequest): Record<string, unknown> {
  const wireBody = buildChatWireBody(request)
  const threadId = request.threadId?.trim() || createRunId('thread')
  const sessionKey = request.sessionKey?.trim() || threadId
  const forwardedProps: Record<string, unknown> = {
    model: request.model,
    profile: request.profile ?? 'chat',
    sessionKey,
    features: wireBody.features,
    generateImage: request.generateImage ?? false,
    browseWeb: request.browseWeb ?? false,
    deepResearch: request.deepResearch ?? false,
    planMode: request.planMode ?? false,
    attachments: request.attachments ?? [],
    zdr: request.zdr ?? false,
  }
  // Keep the AG-UI transport a lossless adapter for the same governed chat
  // request. These fields are optional in the public RunAgentInput shape, so
  // omit them when the user did not select them rather than manufacturing
  // defaults that change provider routing or privacy policy.
  for (const [key, value] of [
    ['effort', request.effort],
    ['spaceRef', request.spaceRef],
    ['mentionedAgentRef', request.mentionedAgentRef],
    ['supportContextQuery', wireBody.support_context_query],
    ['minPrivacyTier', wireBody.min_privacy_tier],
  ] as const) {
    if (value !== undefined && value !== null && value !== '') forwardedProps[key] = value
  }
  const tools = Array.isArray(wireBody.tools)
    ? wireBody.tools
      .filter((tool): tool is Record<string, unknown> => Boolean(tool) && typeof tool === 'object')
      .map((tool) => ({
        name: str(tool.name) ?? '',
        description: str(tool.description) ?? '',
        parameters: parseParametersJson(tool.parameters_json),
      }))
      .filter((tool) => tool.name.length > 0)
    : []

  return {
    threadId,
    runId: createRunId('run'),
    state: {},
    messages: [
      {
        id: createRunId('msg'),
        role: 'user',
        content: request.content,
      },
    ],
    tools,
    context: [],
    forwardedProps,
    data: { ...forwardedProps },
  }
}

export function parseAgUiSseEvent(event: SseEvent): AgUiEvent | null {
  if (!event.data) return null

  try {
    const payload = JSON.parse(event.data) as Record<string, unknown>
    const type = str(payload.type)
    if (!type) return null

    return {
      type,
      timestamp: str(payload.timestamp),
      metadata: objectValue(payload.metadata),
      rawEvent: payload.rawEvent,
      source: str(payload.source),
      runId: str(payload.runId),
      parentRunId: str(payload.parentRunId),
      requestId: str(payload.requestId),
      threadId: str(payload.threadId),
      messageId: str(payload.messageId),
      parentMessageId: str(payload.parentMessageId),
      role: str(payload.role),
      delta: typeof payload.delta === 'string' || Array.isArray(payload.delta)
        ? payload.delta as string | unknown[]
        : undefined,
      content: payload.content,
      snapshot: payload.snapshot,
      patch: Array.isArray(payload.patch)
        ? payload.patch
        : Array.isArray(payload.delta) ? payload.delta : undefined,
      messages: Array.isArray(payload.messages) ? payload.messages : undefined,
      activityType: str(payload.activityType),
      replace: typeof payload.replace === 'boolean' ? payload.replace : undefined,
      event: payload.event,
      outcome: payload.outcome,
      input: payload.input,
      message: str(payload.message),
      code: str(payload.code),
      toolCallId: str(payload.toolCallId),
      toolCallName: str(payload.toolCallName),
      stepName: str(payload.stepName),
      agentId: str(payload.agentId),
      subagentRunId: str(payload.subagentRunId),
      args: payload.args,
      name: str(payload.name),
      value: payload.value,
      modelUsed: str(payload.modelUsed),
      outputTokens: num(payload.outputTokens),
    }
  } catch {
    return null
  }
}

function dispatchAgUiEvent(event: AgUiEvent, handlers: AgUiStreamHandlers): void {
  handlers.onEvent?.(event)

  switch (event.type) {
    case 'RUN_STARTED':
      handlers.onRunStarted?.(event)
      break
    case 'RUN_FINISHED':
      handlers.onRunFinished?.(event)
      break
    case 'RUN_ERROR':
      handlers.onRunError?.(event)
      break
    case 'TEXT_MESSAGE_CONTENT':
    case 'TEXT_MESSAGE_CHUNK':
      handlers.onTextMessageContent?.(event)
      break
    case 'STEP_STARTED':
    case 'STEP_FINISHED':
      handlers.onStep?.(event)
      break
    case 'REASONING_MESSAGE_CONTENT':
      handlers.onReasoning?.(event)
      break
    case 'SUBAGENT_STARTED':
    case 'SUBAGENT_FINISHED':
    case 'SUBAGENT_ERROR':
      handlers.onSubagent?.(event)
      break
    case 'TOOL_CALL_START':
    case 'TOOL_CALL_ARGS':
    case 'TOOL_CALL_END':
    case 'TOOL_CALL_RESULT':
    case 'TOOL_CALL_CHUNK':
      handlers.onToolCall?.(event)
      break
    case 'CUSTOM':
      handlers.onCustom?.(event)
      break
  }
}

/**
 * Dispatch a reverse-serialized, canonical Verevon CUSTOM event through the
 * same focused callbacks as the legacy gateway aliases. The dotted names are
 * intentionally handled only after `toVerevonUiEvent` has accepted the
 * explicit allowlist; arbitrary CUSTOM payloads remain opaque.
 */
function dispatchCanonicalCustomEvent(event: AgUiEvent, handlers: ChatStreamHandlers): boolean {
  const name = event.name?.trim()
  if (!name || !name.includes('.')) return false
  const projected = toVerevonUiEvent(
    'CUSTOM',
    { name, value: event.value },
    event.timestamp,
  )
  if (projected.type === 'unknown') return false

  handlers.onUiEvent?.(projected)
  switch (projected.type) {
    case 'artifact.updated':
      handlers.onArtifact?.({
        id: projected.id,
        kind: projected.kind,
        title: projected.title,
        content: str(objectValue(event.value)?.content),
        version: projected.version,
      })
      break
    case 'attachment.created':
      handlers.onAttachment?.({
        id: projected.id,
        name: projected.name,
        mime: projected.mime,
        type: projected.typeName,
        url: projected.url,
        size: projected.size,
      })
      break
    case 'citation.added':
      handlers.onCitation?.({
        id: projected.id,
        title: projected.title,
        url: projected.url,
        snippet: projected.snippet,
        claimId: projected.claimId,
        sourceGroupId: projected.sourceGroupId,
        start: projected.start,
        end: projected.end,
      })
      break
    case 'grounding.updated':
      handlers.onGrounding?.({ value: projected.value })
      break
    case 'step.updated':
      handlers.onStep?.({ id: projected.id, title: projected.title, detail: projected.detail, status: projected.status })
      break
    case 'delegation.updated':
      handlers.onStep?.({ id: projected.childRunId ?? projected.runId, title: 'Underagent', detail: projected.detail, status: projected.status })
      break
    case 'usage.recorded':
      handlers.onUsage?.({
        inputTokens: projected.inputTokens,
        outputTokens: projected.outputTokens,
        latencyMs: projected.latencyMs,
        costUsd: projected.costUsd,
        confidence: projected.confidence,
        cacheReadTokens: projected.cacheReadTokens,
        cacheWriteTokens: projected.cacheWriteTokens,
      })
      break
    case 'memory.recalled':
      handlers.onMemoryRecall?.({ count: projected.count, latencyMs: projected.latencyMs, memories: [] })
      break
    case 'input.queued':
      handlers.onQueuedInput?.({ messages: projected.messages })
      break
    case 'thread.titled':
      handlers.onTitle?.({ title: projected.title })
      break
    case 'followups.suggested':
      handlers.onFollowUps?.({ suggestions: projected.suggestions })
      break
    case 'run.stopped':
      handlers.onStopped?.({ requestId: projected.requestId, reason: projected.reason })
      break
    // Lifecycle/effect events have no focused callback. `onUiEvent` above is
    // the authoritative path for pause, approval, receipt, cancellation, and
    // trace projections.
    default:
      break
  }
  return true
}

export function dispatchAgUiChatEvent(event: AgUiEvent, handlers: ChatStreamHandlers): void {
  switch (event.type) {
    case 'RUN_STARTED':
      handlers.onConnected?.({
        ok: true,
        requestId: event.requestId ?? event.runId,
        threadId: event.threadId,
        runId: event.runId,
        model: event.modelUsed,
      })
      break
    case 'TEXT_MESSAGE_CONTENT':
    case 'TEXT_MESSAGE_CHUNK':
      if (typeof event.delta === 'string' && event.delta) {
        handlers.onMessage?.({ content: event.delta, requestId: event.requestId ?? event.runId })
      }
      break
    case 'STEP_STARTED':
    case 'STEP_FINISHED':
      handlers.onStep?.({
        id: event.stepName ?? event.name,
        title: event.stepName ?? event.name,
        detail: event.message,
        status: event.type === 'STEP_STARTED' ? 'started' : 'finished',
      })
      break
    case 'REASONING_MESSAGE_CONTENT':
      if (typeof event.delta === 'string' && event.delta) handlers.onReasoning?.({ delta: event.delta })
      break
    case 'RUN_FINISHED':
      {
        const outcome = objectValue(event.outcome)
        if (String(outcome?.type ?? '').toLowerCase() === 'interrupt') {
          // An AG-UI interrupt is a resumable pause, not a successful answer.
          // Preserve it in the canonical event lane so approval/Work controls
          // remain visible to callers that consume this adapter directly.
          handlers.onUiEvent?.({
            type: 'run.paused',
            at: event.timestamp ?? new Date().toISOString(),
            runId: event.runId,
            interrupts: Array.isArray(outcome?.interrupts) ? outcome.interrupts : undefined,
          })
        } else if (String(outcome?.status ?? '').toLowerCase() === 'stopped' || String(outcome?.status ?? '').toLowerCase() === 'cancelled') {
          handlers.onStopped?.({
            requestId: event.requestId ?? event.runId,
            reason: event.message,
          })
        } else {
          handlers.onDone?.({
            requestId: event.requestId ?? event.runId,
            modelUsed: event.modelUsed,
            outputTokens: event.outputTokens,
          })
        }
      }
      break
    case 'RUN_ERROR':
      handlers.onError?.({
        code: event.code ?? 'error',
        message: event.message ?? 'Stream error',
        runId: event.runId,
        requestId: event.requestId,
      })
      break
    case 'TOOL_CALL_START':
    case 'TOOL_CALL_ARGS':
    case 'TOOL_CALL_CHUNK':
      handlers.onToolCall?.({
        id: event.toolCallId,
        name: event.toolCallName,
        args: argumentValue(event),
      })
      break
    case 'SUBAGENT_STARTED':
    case 'SUBAGENT_FINISHED':
    case 'SUBAGENT_ERROR':
      handlers.onStep?.({
        id: event.runId,
        title: 'Underagent',
        detail: event.message ?? event.agentId,
        status: event.type === 'SUBAGENT_STARTED' ? 'started' : event.type === 'SUBAGENT_ERROR' ? 'error' : 'finished',
      })
      break
    case 'TOOL_CALL_END':
      handlers.onToolResult?.({
        id: event.toolCallId,
        status: 'done',
      })
      break
    case 'TOOL_CALL_RESULT':
      handlers.onToolResult?.({
        id: event.toolCallId,
        output: textValue(event.content),
        status: 'done',
      })
      break
    case 'CUSTOM': {
      const value = objectValue(event.value)
      if (!value) return

      if (dispatchCanonicalCustomEvent(event, handlers)) break

      if (event.name === 'artifact') {
        handlers.onArtifact?.({
          id: str(value.id),
          kind: str(value.kind),
          title: str(value.title),
          content: str(value.content),
          version: num(value.version),
        })
      } else if (event.name === 'attachment') {
        handlers.onAttachment?.({
          id: str(value.id),
          name: str(value.name),
          mime: str(value.mime),
          type: str(value.type),
          url: str(value.url),
          size: num(value.size),
        })
      } else if (event.name === 'reasoning_delta') {
        handlers.onReasoning?.({ delta: str(value.delta) ?? '' })
      } else if (event.name === 'citation') {
        handlers.onCitation?.({
          id: str(value.id),
          title: str(value.title),
          url: str(value.url) ?? str(value.href),
          snippet: str(value.snippet) ?? str(value.description),
          claimId: str(value.claim_id) ?? str(value.claimId),
          sourceGroupId: str(value.source_group_id) ?? str(value.sourceGroupId),
          start: num(value.start) ?? num(value.start_offset) ?? num(value.startOffset),
          end: num(value.end) ?? num(value.end_offset) ?? num(value.endOffset),
        })
      } else if (event.name === 'grounding') {
        handlers.onGrounding?.({ value: value.grounding ?? value })
      } else if (event.name === 'step_update') {
        handlers.onStep?.({
          id: str(value.id) ?? str(value.step_id),
          title: str(value.title) ?? str(value.name),
          detail: str(value.detail) ?? str(value.message),
          status: str(value.status),
        })
      } else if (event.name === 'tool_result') {
        handlers.onToolResult?.({
          id: str(value.id) ?? str(value.tool_call_id),
          output: str(value.output) ?? str(value.result),
          error: str(value.error),
          status: str(value.status),
        })
      } else if (event.name === 'usage') {
        handlers.onUsage?.({
          inputTokens: num(value.input_tokens),
          outputTokens: num(value.output_tokens),
          costUsd: num(value.cost_usd),
          latencyMs: num(value.latency_ms),
          confidence: num(value.confidence),
          cacheReadTokens: num(value.cache_read_tokens) ?? num(value.cacheReadTokens),
          cacheWriteTokens: num(value.cache_write_tokens) ?? num(value.cacheWriteTokens),
        })
      }
      break
    }
  }
}

export async function streamAgentUi(
  request: ChatInvokeRequest,
  handlers: AgUiStreamHandlers,
  signal?: AbortSignal,
  lastEventId?: string,
): Promise<void> {
  let connError: unknown

  await readSseStream(
    '/api/v1/ag-ui/stream',
    {
      method: 'POST',
      body: JSON.stringify(buildAgentRunInputBody(request)),
      signal,
      lastEventId,
    },
    (event) => {
      if (event.id) handlers.onFrameId?.(event.id)
      const parsed = parseAgUiSseEvent(event)
      if (parsed) dispatchAgUiEvent(parsed, handlers)
    },
    (err) => {
      connError = err
    },
  )

  if (connError) {
    const msg = connError instanceof Error ? connError.message : 'Connection failed'
    handlers.onRunError?.({ type: 'RUN_ERROR', message: msg, code: 'connection_error' })
  }
}

/** Replay a previously-started AG-UI run without re-dispatching the user
 * message. The gateway transforms Model Gateway's native replay frames at the
 * same `/stream/:request_id` boundary and honours `Last-Event-ID`. */
export async function resumeAgentUi(
  requestId: string,
  handlers: AgUiStreamHandlers,
  signal?: AbortSignal,
  lastEventId?: string,
): Promise<void> {
  const normalizedRequestId = requestId.trim()
  if (!normalizedRequestId) return
  const encoded = encodeURIComponent(normalizedRequestId)
  let connError: unknown

  await readSseStream(
    `/api/v1/ag-ui/stream/${encoded}`,
    { method: 'GET', signal, lastEventId },
    (event) => {
      if (event.id) handlers.onFrameId?.(event.id)
      const parsed = parseAgUiSseEvent(event)
      if (parsed) dispatchAgUiEvent(parsed, handlers)
    },
    (err) => {
      connError = err
    },
  )

  if (connError) {
    const msg = connError instanceof Error ? connError.message : 'Connection failed'
    handlers.onRunError?.({ type: 'RUN_ERROR', message: msg, code: 'connection_error' })
  }
}

export async function streamAgentChat(
  request: ChatInvokeRequest,
  handlers: ChatStreamHandlers,
  signal?: AbortSignal,
  lastEventId?: string,
): Promise<void> {
  await streamAgentUi(
    request,
    {
      onEvent: (event) => dispatchAgUiChatEvent(event, handlers),
      onRunError: (event) => {
        if (event.code === 'connection_error') {
          dispatchAgUiChatEvent(event, handlers)
        }
      },
      onFrameId: handlers.onFrameId,
    },
    signal,
    lastEventId,
  )
}
