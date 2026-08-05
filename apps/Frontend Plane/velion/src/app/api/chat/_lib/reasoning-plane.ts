import { z } from 'zod'
import { invokeReasoning, invokeReasoningStream } from '@/lib/model-plane/reasoning'

import type { ChatMessageMetadata, StoredChatMessage } from './session-store'
const MAX_MESSAGE_LENGTH = 8_000
const MAX_HISTORY_MESSAGES = 8
const REQUEST_TIMEOUT_MS = 90_000
const SIMPLE_GREETING_PATTERN =
  /^(hei(?:\s+hei)?|hi|hello|hey|hallo|halloi|god\s+(?:morgen|dag|kveld|ettermiddag)|good\s+(?:morning|afternoon|evening))[\s!?.!,;:]*$/iu

const chatRequestSchema = z.object({
  content: z.string().trim().min(1).max(MAX_MESSAGE_LENGTH),
  sessionId: z.string().trim().min(1).max(128).optional(),
  clientId: z.string().trim().min(1).max(128).optional(),
  lastEventId: z.string().trim().min(1).max(256).optional(),
  userId: z.string().trim().min(1).max(256).optional(),
  userName: z.string().trim().min(1).max(256).optional(),
  userEmail: z.string().trim().email().optional(),
  model: z.string().trim().min(1).max(128).optional(),
  agentId: z.string().trim().min(1).max(128).optional(),
  responseMode: z.enum(['auto', 'quick', 'deep']).optional(),
  browseWeb: z.boolean().optional(),
})

const reasoningStepSchema = z
  .object({
    sources: z.array(z.string()).optional(),
  })
  .passthrough()

const reasoningResponseSchema = z
  .object({
    query: z.string(),
    answer: z.string(),
    reasoning_trace: z.array(reasoningStepSchema),
    strategy_used: z.string(),
    confidence: z.number(),
    reasoning_time_ms: z.number(),
    alternative_explanations: z.array(z.string()).optional(),
    verification_result: z.record(z.string(), z.unknown()).optional().nullable(),
    metadata: z.record(z.string(), z.unknown()).optional(),
  })
  .passthrough()

export type ParsedChatRequest = z.infer<typeof chatRequestSchema>

type ReasoningResponse = z.infer<typeof reasoningResponseSchema>

export function parseChatRequest(body: unknown): ParsedChatRequest {
  return chatRequestSchema.parse(body)
}

function truncate(value: string, maxLength: number) {
  return value.length > maxLength ? `${value.slice(0, maxLength - 3)}...` : value
}

function buildConversationContext(history: StoredChatMessage[]) {
  const recentHistory = history.slice(-MAX_HISTORY_MESSAGES)

  if (!recentHistory.length) {
    return ''
  }

  const formattedHistory = recentHistory
    .map((message) => {
      const speaker = message.role === 'assistant' ? 'Assistant' : message.role === 'system' ? 'System' : 'User'
      return `${speaker}: ${truncate(message.content, 600)}`
    })
    .join('\n')

  return [
    'Conversation context:',
    formattedHistory,
    '',
    'Answer the latest user request while keeping the prior conversation in mind when it is relevant.',
    '',
  ].join('\n')
}

function collectCitationHints(response: ReasoningResponse) {
  return Array.from(
    new Set(
      response.reasoning_trace.flatMap((step) => step.sources ?? []).filter((source) => source && source !== 'reasoning_model'),
    ),
  )
}

function isSimpleGreeting(content: string) {
  const normalized = content.trim().replace(/\s+/g, ' ')

  if (!normalized || normalized.length > 40) {
    return false
  }

  return SIMPLE_GREETING_PATTERN.test(normalized)
}

function selectReasoningDepth(content: string, history: StoredChatMessage[]) {
  if (history.length >= 6 || content.length >= 500) {
    return 'deep'
  }

  if (history.length >= 2 || content.length >= 220) {
    return 'standard'
  }

  return 'fast'
}

export async function requestReasoningPlaneAnswer(options: {
  content: string
  history: StoredChatMessage[]
  sessionId: string
  userId?: string
  userName?: string
  userEmail?: string
  model?: string
  responseMode?: 'auto' | 'quick' | 'deep'
  browseWeb?: boolean
  /**
   * U2-5: the caller forwards `request.headers.get('cookie')` here so
   * `invokeReasoning` can mint a real Model Plane JWT against the user's
   * Better Auth session. When absent we fall through to env-var bearer
   * (only works against gateways with MODEL_GATEWAY_AUTH_DEV_BYPASS=1).
   */
  cookieHeader?: string
  /**
   * §15 (ui-ux-verevon-gap.md): when calling on behalf of a configured
   * agent, forward the agent's enabled tool ids. The gateway's
   * `/v1/invoke` runs the tool-use loop when this array is non-empty
   * (`tool_registry::build_registry` + `tool_loop::run_tool_loop`).
   * Tools are `browse_web`, `fetch_url`, `deep_research`,
   * `image_generate`, `skill:{id}`, `integration:{connector}.{op}`.
   */
  tools?: readonly string[]
}) {
  const { content, history, sessionId, userId, userName, userEmail, model, responseMode, browseWeb, cookieHeader, tools } = options
  const greetingOnly = isSimpleGreeting(content)

  const contextPrefix = buildConversationContext(history)
  const latestRequest = greetingOnly
    ? [
        'The latest user message is a greeting.',
        'Reply briefly, warmly, and naturally in the same language as the user.',
        'Do not define the greeting, translate it, or provide dictionary-style background.',
        'Keep the reply to one short sentence unless the user asks for more.',
        '',
        'Latest user request:',
        content,
      ].join('\n')
    : `Latest user request:\n${content}`

  const query = contextPrefix
    ? `${contextPrefix}${latestRequest}`
    : greetingOnly
      ? latestRequest
      : content

  // responseMode overrides auto-depth
  const depthMap = { quick: 'fast', auto: undefined, deep: 'deep' } as const
  const depth = (responseMode && responseMode !== 'auto')
    ? depthMap[responseMode]
    : selectReasoningDepth(content, history)

  const payload = {
    query,
    strategy: 'auto',
    depth,
    context: {
      session_id: sessionId,
      user_id: userId,
      user_name: userName,
      user_email: userEmail,
      history: history.slice(-MAX_HISTORY_MESSAGES).map((message) => ({
        role: message.role,
        content: truncate(message.content, 600),
        timestamp: message.timestamp,
      })),
    },
    require_citations: !greetingOnly,
    enable_verification: !greetingOnly,
    model: model ?? undefined,
    enable_web_search: browseWeb ?? false,
    // §15: forwarded to the gateway as `tools` on the InvokeRequest.
    // `buildRustInvokePayload` (in lib/model-plane/reasoning.ts) reads
    // this field off the request body and shapes the gateway POST.
    tools: tools && tools.length > 0 ? Array.from(tools) : undefined,
  }

  const response = await invokeReasoning(payload, {
    signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS),
    cookieHeader,
  })

  const rawResponse = await response.text()

  if (!response.ok) {
    const detail = rawResponse || `reasoning-core ${response.status}`
    throw new Error(detail)
  }

  let data: unknown = rawResponse
  try {
    data = JSON.parse(rawResponse)
  } catch {
    throw new Error('Invalid reasoning-core response')
  }

  const parsed = reasoningResponseSchema.safeParse(data)
  if (!parsed.success) {
    throw new Error('Reasoning-core response validation failed')
  }

  const result = parsed.data
  const metadata: ChatMessageMetadata = {
    source: 'reasoning-plane',
    citations: collectCitationHints(result),
  }

  // Wave 9 §19: pull the gateway's per-turn tool-loop trace off the
  // normalized envelope. `normalizeRustResponse` (lib/model-plane/
  // reasoning.ts) packs it as `metadata.tool_trace`. Empty when the
  // model never invoked a tool — the caller short-circuits on length.
  const rawMetadata = (result.metadata ?? {}) as Record<string, unknown>
  const rawTrace = rawMetadata['tool_trace']
  const toolTrace = Array.isArray(rawTrace)
    ? (rawTrace as Array<{
        round: number
        tool: string
        args_preview: string
        result_preview: string
        result_bytes: number
      }>)
    : []

  // Wave 11 §5 — pull the gateway's run id off `metadata.runId`
  // (camelCase, set by `normalizeRustResponse`). Used by the stream
  // route to emit a trailing `run_id` SSE envelope so the playground
  // hook can attach Fin G/A/P ratings.
  const runIdRaw = rawMetadata['runId']
  const runId = typeof runIdRaw === 'string' && runIdRaw ? runIdRaw : undefined

  return {
    answer: result.answer.trim(),
    metadata,
    toolTrace,
    runId,
    raw: result,
  }
}

export function createAnswerChunks(answer: string) {
  const normalized = answer.trim()
  const chunks = normalized.match(/\S+\s*/g)

  if (!chunks?.length) {
    return [normalized]
  }

  return chunks
}

/**
 * Streaming counterpart to `requestReasoningPlaneAnswer`. Yields each
 * delta from the gateway's `/v1/invoke/stream` SSE endpoint plus a
 * terminal `done` envelope. Caller is responsible for accumulating
 * the deltas into the final answer and writing them to Convex.
 *
 * Behaviour matches the non-streaming path's request construction
 * (conversation context prefix, greeting heuristic, depth selection,
 * timeout) so the two paths produce comparable outputs given the
 * same input — only the delivery shape differs.
 *
 * The orchestrator-level features (citations, verification metadata,
 * tool-loop traces, deep-research synthesis) are NOT available on the
 * SSE path because it bypasses orchestrator-core. Callers that need
 * those features MUST fall back to `requestReasoningPlaneAnswer`.
 */
export async function* streamReasoningPlaneAnswer(options: {
  content: string
  history: StoredChatMessage[]
  sessionId: string
  userId?: string
  userName?: string
  userEmail?: string
  model?: string
  responseMode?: 'auto' | 'quick' | 'deep'
  profile?: 'chat' | 'deployed_agent'
  cookieHeader?: string
  signal?: AbortSignal
}): AsyncGenerator<
  | { type: 'meta'; requestId: string }
  | { type: 'delta'; delta: string }
  | { type: 'done'; modelUsed: string; inputTokens: number; outputTokens: number },
  void,
  void
> {
  const { content, history, sessionId, userId, userName, userEmail, model, profile, cookieHeader, signal } =
    options
  const greetingOnly = isSimpleGreeting(content)

  const contextPrefix = buildConversationContext(history)
  const latestRequest = greetingOnly
    ? [
        'The latest user message is a greeting.',
        'Reply briefly, warmly, and naturally in the same language as the user.',
        'Do not define the greeting, translate it, or provide dictionary-style background.',
        'Keep the reply to one short sentence unless the user asks for more.',
        '',
        'Latest user request:',
        content,
      ].join('\n')
    : `Latest user request:\n${content}`

  const query = contextPrefix
    ? `${contextPrefix}${latestRequest}`
    : greetingOnly
      ? latestRequest
      : content

  const payload: ReasoningRequest = {
    query,
    strategy: 'auto',
    depth: selectReasoningDepth(content, history),
    context: {
      session_id: sessionId,
      user_id: userId,
      user_name: userName,
      user_email: userEmail,
    },
    require_citations: !greetingOnly,
    enable_verification: !greetingOnly,
    model: model ?? undefined,
    profile,
  }

  // Compose timeout + caller-supplied abort. If either fires, the
  // underlying fetch is aborted and the generator throws.
  const timeoutController = new AbortController()
  const timeoutId = setTimeout(() => timeoutController.abort(), REQUEST_TIMEOUT_MS)
  const combined = combineSignals([signal, timeoutController.signal])

  try {
    for await (const event of invokeReasoningStream(payload, {
      signal: combined,
      cookieHeader,
    })) {
      yield event
    }
  } finally {
    clearTimeout(timeoutId)
  }
}

type ReasoningRequest = Parameters<typeof invokeReasoningStream>[0]

function combineSignals(signals: Array<AbortSignal | undefined>): AbortSignal {
  const controller = new AbortController()
  for (const sig of signals) {
    if (!sig) continue
    if (sig.aborted) {
      controller.abort()
      break
    }
    sig.addEventListener('abort', () => controller.abort(), { once: true })
  }
  return controller.signal
}
