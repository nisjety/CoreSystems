import { NextRequest, NextResponse } from 'next/server'
import { z } from 'zod'

import { resolveChatActor } from '@/app/api/chat/_lib/session-store'

/**
 * POST /api/inbox/draft/feedback
 *
 * Stores an approved AI draft as an example in agent-core's memory.
 * The next draft generation will inject these examples into the system prompt,
 * giving the model brand-specific patterns to follow.
 *
 * Body:
 *   agentId       - which agent produced the draft
 *   customerName  - display name (for context)
 *   customerMessage - the customer's message that triggered the draft
 *   approvedText  - the final text the agent sent (may differ from original AI draft)
 *
 * Fires-and-forgets to agent-core memory.  Always returns 200 so the
 * caller does not need to wait or handle errors.
 */

const AGENT_CORE_URL = process.env.AGENT_CORE_URL || 'http://localhost:8002'
const INTERNAL_API_KEY = process.env.INTERNAL_API_KEY ?? process.env.INTERNAL_SERVICE_SECRET ?? ''

const feedbackSchema = z.object({
  agentId: z.string().min(1).max(128),
  customerName: z.string().max(128).optional(),
  customerMessage: z.string().max(2_000).optional(),
  approvedText: z.string().min(1).max(4_000),
})

export async function POST(request: NextRequest) {
  try {
    const actor = await resolveChatActor()
    const body = await request.json()
    const { agentId, customerName, customerMessage, approvedText } = feedbackSchema.parse(body)

    // Format as a structured example for future prompt injection
    const entryLines = [
      `[Inbox Draft Example — agentId: ${agentId}]`,
      customerName ? `Customer: ${customerName}` : null,
      customerMessage ? `Message: ${customerMessage}` : null,
      `Approved reply: ${approvedText}`,
    ].filter(Boolean)

    const content = entryLines.join('\n')
    const key = `inbox:draft:${agentId}:${Date.now()}`

    // Write to agent-core memory — non-blocking best-effort
    await fetch(`${AGENT_CORE_URL}/v1/memory`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'X-Internal-Api-Key': INTERNAL_API_KEY,
        'X-Org-Id': actor.convexOrgId,
        'X-User-Id': actor.userId,
      },
      body: JSON.stringify({ key, content }),
      signal: AbortSignal.timeout(5_000),
    }).catch(() => undefined) // swallow — feedback failure must not surface to user

    return NextResponse.json({ accepted: true })
  } catch {
    // Return 200 even on error so the caller's fire-and-forget doesn't need handling
    return NextResponse.json({ accepted: false })
  }
}
