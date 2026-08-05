import {
  buildChatWireBody,
  type ChatInvokeRequest,
  type ChatStreamHandlers,
} from './chat-client'
import { readSseStream, type SseEvent } from './sse'

export const AG_UI_EVENT_TYPES = [
  'RUN_STARTED',
  'RUN_FINISHED',
  'RUN_ERROR',
  'TEXT_MESSAGE_START',
  'TEXT_MESSAGE_CONTENT',
  'TEXT_MESSAGE_END',
  'TOOL_CALL_START',
  'TOOL_CALL_ARGS',
  'TOOL_CALL_END',
  'TOOL_CALL_RESULT',
  'STATE_SNAPSHOT',
  'STATE_DELTA',
  'MESSAGES_SNAPSHOT',
  'CUSTOM',
] as const

export type AgUiEventType = (typeof AG_UI_EVENT_TYPES)[number]

export type AgUiEvent = {
  type: AgUiEventType | string
  runId?: string
  threadId?: string
  messageId?: string
  role?: string
  delta?: string
  content?: unknown
  message?: string
  code?: string
  toolCallId?: string
  toolCallName?: string
  args?: unknown
  name?: string
  value?: unknown
  modelUsed?: string
  outputTokens?: number
}

export type AgUiStreamHandlers = {
  onEvent?: (event: AgUiEvent) => void
  onRunStarted?: (event: AgUiEvent) => void
  onRunFinished?: (event: AgUiEvent) => void
  onRunError?: (event: AgUiEvent) => void
  onTextMessageContent?: (event: AgUiEvent) => void
  onToolCall?: (event: AgUiEvent) => void
  onCustom?: (event: AgUiEvent) => void
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
  const forwardedProps = {
    model: request.model,
    profile: request.profile ?? 'chat',
    sessionKey,
    features: wireBody.features,
    generateImage: request.generateImage ?? false,
    browseWeb: request.browseWeb ?? false,
    attachments: request.attachments ?? [],
    zdr: request.zdr ?? false,
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
      runId: str(payload.runId),
      threadId: str(payload.threadId),
      messageId: str(payload.messageId),
      role: str(payload.role),
      delta: str(payload.delta),
      content: payload.content,
      message: str(payload.message),
      code: str(payload.code),
      toolCallId: str(payload.toolCallId),
      toolCallName: str(payload.toolCallName),
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
      handlers.onTextMessageContent?.(event)
      break
    case 'TOOL_CALL_START':
    case 'TOOL_CALL_ARGS':
    case 'TOOL_CALL_END':
    case 'TOOL_CALL_RESULT':
      handlers.onToolCall?.(event)
      break
    case 'CUSTOM':
      handlers.onCustom?.(event)
      break
  }
}

export function dispatchAgUiChatEvent(event: AgUiEvent, handlers: ChatStreamHandlers): void {
  switch (event.type) {
    case 'RUN_STARTED':
      handlers.onConnected?.({
        ok: true,
        requestId: event.runId,
        threadId: event.threadId,
        model: event.modelUsed,
      })
      break
    case 'TEXT_MESSAGE_CONTENT':
      if (event.delta) {
        handlers.onMessage?.({ content: event.delta, requestId: event.runId })
      }
      break
    case 'RUN_FINISHED':
      handlers.onDone?.({
        requestId: event.runId,
        modelUsed: event.modelUsed,
        outputTokens: event.outputTokens,
      })
      break
    case 'RUN_ERROR':
      handlers.onError?.({
        code: event.code ?? 'error',
        message: event.message ?? 'Stream error',
      })
      break
    case 'TOOL_CALL_START':
    case 'TOOL_CALL_ARGS':
      handlers.onToolCall?.({
        id: event.toolCallId,
        name: event.toolCallName,
        args: event.args,
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
    },
    signal,
    lastEventId,
  )
}
