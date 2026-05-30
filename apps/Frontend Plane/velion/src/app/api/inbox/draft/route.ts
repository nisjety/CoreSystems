import { NextRequest } from 'next/server'
import { z } from 'zod'

import { resolveChatActor, ChatStoreError } from '@/app/api/chat/_lib/session-store'
import { convexQuery } from '@/app/api/_lib/convex-client'
import type { PersistedAgent } from '@/components/agents/types'
import { invokeReasoning } from '@/lib/model-plane/reasoning'

const AGENT_CORE_URL = process.env.AGENT_CORE_URL || 'http://localhost:8002'
const INTERNAL_API_KEY = process.env.INTERNAL_API_KEY ?? process.env.INTERNAL_SERVICE_SECRET ?? ''
const REQUEST_TIMEOUT_MS = 60_000

const draftRequestSchema = z.object({
  conversationHistory: z.array(
    z.object({
      role: z.enum(['user', 'assistant']),
      content: z.string().max(4_000),
    }),
  ).max(20),
  customerName: z.string().max(128).optional(),
  agentId: z.string().optional(),
})

/** Fetch approved-draft examples for a given agentId from agent-core memory. */
async function fetchDraftExamples(orgId: string, userId: string, agentId: string): Promise<string[]> {
  try {
    const res = await fetch(`${AGENT_CORE_URL}/v1/memory`, {
      headers: {
        'X-Internal-Api-Key': INTERNAL_API_KEY,
        'X-Org-Id': orgId,
        'X-User-Id': userId,
      },
      signal: AbortSignal.timeout(3_000),
    })
    if (!res.ok) return []
    const entries: Array<{ key: string; content: string }> = await res.json()
    return entries
      .filter((e) => e.key.startsWith(`inbox:draft:${agentId}:`))
      .map((e) => e.content)
      .slice(-3) // at most 3 recent examples
  } catch {
    return []
  }
}

function buildDraftSystemPrompt(
  customerName: string | undefined,
  agentSystemPrompt: string | undefined,
  agentTone: string | undefined,
  examples?: string[],
): string {
  const parts: string[] = []

  if (agentSystemPrompt) {
    parts.push(agentSystemPrompt)
    parts.push('')
  }

  parts.push(
    'You are a customer support agent composing a reply.',
    `${customerName ? `The customer you are replying to is ${customerName}.` : ''}`,
    'Based on the conversation history, draft a helpful, concise, and empathetic reply.',
    'Write in the same language as the customer.',
    'Be direct and action-oriented. Do not use filler phrases.',
    agentTone ? `Tone: ${agentTone}.` : '',
    '',
    'Return ONLY the reply text. Do not include greetings like "Hi [Name]," unless they appear naturally in the conversation style.',
  )

  if (examples && examples.length > 0) {
    parts.push(
      '',
      '--- Past approved replies for this agent (use as style reference) ---',
      ...examples,
      '---',
    )
  }

  return parts.filter(Boolean).join('\n')
}

function buildHistoryContext(
  history: Array<{ role: 'user' | 'assistant'; content: string }>,
): string {
  if (!history.length) return ''

  return history
    .map((m) => `${m.role === 'user' ? 'Customer' : 'Agent'}: ${m.content}`)
    .join('\n')
}

export async function POST(request: NextRequest) {
  const encoder = new TextEncoder()

  const sendChunk = (content: string) =>
    encoder.encode(`data: ${JSON.stringify({ type: 'chunk', content })}\n\n`)
  const sendDone = () => encoder.encode('data: [DONE]\n\n')

  try {
    const actor = await resolveChatActor()
    const body = await request.json()
    const { conversationHistory, customerName, agentId } = draftRequestSchema.parse(body)

    // Optionally load agent config for system prompt + tone
    let agentConfig: PersistedAgent | null = null
    if (agentId) {
      agentConfig = await convexQuery<PersistedAgent | null>('agents:getById', {
        agentId,
        orgId: actor.convexOrgId,
      }).catch(() => null)
    }

    // Load approved-draft examples for this agent (best-effort — empty on failure)
    const draftExamples = agentId
      ? await fetchDraftExamples(actor.convexOrgId, actor.userId, agentId)
      : []

    const systemPrompt = buildDraftSystemPrompt(
      customerName,
      agentConfig?.systemPrompt,
      agentConfig?.tone,
      draftExamples,
    )
    const historyContext = buildHistoryContext(conversationHistory)

    const query = historyContext
      ? `${historyContext}\n\nDraft a reply to the customer's latest message.`
      : 'Draft a helpful reply to start this customer conversation.'

    const payload = {
      query,
      strategy: 'fast',
      depth: 'fast',
      context: {
        session_id: `inbox-draft-${actor.convexUserId}-${Date.now()}`,
        user_id: actor.userId,
        system_prompt: systemPrompt,
      },
      require_citations: false,
      enable_verification: false,
      model: agentConfig?.model ?? undefined,
    }

    const stream = new ReadableStream({
      async start(controller) {
        try {
          const response = await invokeReasoning(payload, {
            signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS),
            // U2-5: forward user session so the gateway gets a real JWT.
            cookieHeader: request.headers.get('cookie') ?? '',
          })

          if (!response.ok) {
            const detail = await response.text()
            throw new Error(detail || `reasoning-core ${response.status}`)
          }

          const data = await response.json()
          const answer: string = data?.answer ?? ''

          // Stream word by word for a typing effect
          const words = answer.trim().match(/\S+\s*/g) ?? [answer]
          for (const word of words) {
            controller.enqueue(sendChunk(word))
            await new Promise((r) => setTimeout(r, 10))
          }

          controller.enqueue(sendDone())
        } catch (err) {
          const msg = err instanceof Error ? err.message : 'Draft generation failed'
          controller.enqueue(
            encoder.encode(`data: ${JSON.stringify({ type: 'error', error: msg })}\n\n`),
          )
        } finally {
          controller.close()
        }
      },
    })

    return new Response(stream, {
      headers: {
        'Content-Type': 'text/event-stream',
        'Cache-Control': 'no-cache',
        Connection: 'keep-alive',
      },
    })
  } catch (error) {
    if (error instanceof ChatStoreError) {
      return Response.json(
        { error: error.message },
        { status: error.statusCode },
      )
    }

    return Response.json(
      { error: error instanceof Error ? error.message : 'Internal server error' },
      { status: error instanceof Error && error.message.includes('Validation') ? 400 : 500 },
    )
  }
}
