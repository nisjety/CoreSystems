import { NextRequest, NextResponse } from 'next/server'
import { z } from 'zod'

import { resolveChatActor, ChatStoreError } from '@/app/api/chat/_lib/session-store'
import { convexMutation } from '@/app/api/_lib/convex-client'

const bindingsSchema = z.object({
  scope: z.enum(['all', 'selected']),
  documentIds: z.array(z.string()).optional(),
  sourceIds: z.array(z.string()).optional(),
  includeQnA: z.boolean().optional(),
})

interface RouteContext {
  params: Promise<{ agentId: string }>
}

/**
 * Wave 11 §6 — per-agent knowledge-binding scope. Routes operator
 * picks from the AgentKnowledgeBindings chip-picker → Convex
 * `agents:updateKnowledgeBindings`. Gateway picks up the new scope on
 * the next retrieval call.
 */
export async function PUT(request: NextRequest, context: RouteContext) {
  try {
    const { agentId } = await context.params
    const actor = await resolveChatActor()
    const body = await request.json()
    const parsed = bindingsSchema.parse(body)

    const updated = await convexMutation<unknown>('agents:updateKnowledgeBindings', {
      agentId,
      orgId: actor.convexOrgId,
      ...parsed,
    })

    return NextResponse.json({ success: true, agent: updated })
  } catch (error) {
    if (error instanceof ChatStoreError) {
      return NextResponse.json({ error: error.message }, { status: error.statusCode })
    }
    if (error && typeof error === 'object' && 'issues' in error) {
      const issues = (error as { issues: Array<{ message?: string }> }).issues
      return NextResponse.json(
        { error: issues[0]?.message ?? 'Validation failed' },
        { status: 400 },
      )
    }
    return NextResponse.json(
      { error: error instanceof Error ? error.message : 'Update failed' },
      { status: 500 },
    )
  }
}
