import { z } from 'zod'

import type { ChatMessageMetadata, StoredChatMessage } from './session-store'

const REASONING_CORE_URL = process.env.REASONING_CORE_URL || 'http://localhost:8101'
const MAX_MESSAGE_LENGTH = 8_000
const MAX_HISTORY_MESSAGES = 8
const REQUEST_TIMEOUT_MS = 90_000
const SIMPLE_GREETING_PATTERN =
  /^(hei(?:\s+hei)?|hi|hello|hey|hallo|halloi|god\s+(?:morgen|dag|kveld|ettermiddag)|good\s+(?:morning|afternoon|evening))[\s!?.!,;:]*$/iu

const chatRequestSchema = z.object({
  content: z.string().trim().min(1).max(MAX_MESSAGE_LENGTH),
  sessionId: z.string().trim().min(1).max(128).optional(),
  userId: z.string().trim().min(1).max(256).optional(),
  userName: z.string().trim().min(1).max(256).optional(),
  userEmail: z.string().trim().email().optional(),
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
}) {
  const { content, history, sessionId, userId, userName, userEmail } = options
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

  const payload = {
    query,
    strategy: 'auto',
    depth: selectReasoningDepth(content, history),
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
  }

  const response = await fetch(`${REASONING_CORE_URL}/api/v1/reason`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
    },
    body: JSON.stringify(payload),
    signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS),
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

  return {
    answer: result.answer.trim(),
    metadata,
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
