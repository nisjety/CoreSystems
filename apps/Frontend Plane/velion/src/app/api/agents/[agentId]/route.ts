import { NextRequest, NextResponse } from 'next/server'
import { ZodError, z } from 'zod'

import { resolveChatActor } from '@/app/api/chat/_lib/session-store'
import { convexMutation, convexQuery } from '@/app/api/_lib/convex-client'
import type { PersistedAgent } from '@/components/agents/types'

const updateAgentSchema = z.object({
  name: z.string().trim().min(1).max(128).optional(),
  description: z.string().max(500).optional(),
  useCase: z.enum([
    'customer_support', 'sales', 'marketing', 'hr', 'faq', 'onboarding', 'other',
  ]).optional(),
  status: z.enum(['active', 'inactive', 'draft']).optional(),
  profile: z.enum(['chat', 'deployed_agent']).optional(),
  model: z.string().trim().min(1).max(128).optional(),
  temperature: z.number().min(0).max(1).optional(),
  systemPrompt: z.string().max(10_000).optional(),
  tone: z.string().max(128).optional(),
  greeting: z.string().max(500).optional(),
  tools: z.array(z.string()).optional(),
  knowledgeSources: z.array(z.object({
    type: z.string(),
    name: z.string(),
    id: z.string().optional(),
  })).optional(),
})

interface RouteContext {
  params: Promise<{ agentId: string }>
}

export async function GET(_request: NextRequest, { params }: RouteContext) {
  try {
    const { agentId } = await params
    const actor = await resolveChatActor()

    const agent = await convexQuery<PersistedAgent | null>('agents:getById', {
      agentId,
      orgId: actor.convexOrgId,
    })

    if (!agent) {
      return NextResponse.json({ success: false, error: 'Agent not found' }, { status: 404 })
    }

    return NextResponse.json({ success: true, data: agent })
  } catch (error) {
    return errorResponse(error)
  }
}

export async function PATCH(request: NextRequest, { params }: RouteContext) {
  try {
    const { agentId } = await params
    const actor = await resolveChatActor()
    const body = await request.json()
    const parsed = updateAgentSchema.parse(body)

    await convexMutation('agents:update', {
      agentId,
      orgId: actor.convexOrgId,
      ...parsed,
    })

    const agent = await convexQuery<PersistedAgent | null>('agents:getById', {
      agentId,
      orgId: actor.convexOrgId,
    })

    return NextResponse.json({ success: true, data: agent })
  } catch (error) {
    return errorResponse(error)
  }
}

function errorResponse(error: unknown) {
  if (error instanceof ZodError) {
    return NextResponse.json(
      { success: false, error: error.issues[0]?.message ?? 'Validation failed' },
      { status: 400 },
    )
  }

  if (error instanceof Error && error.message.includes('Authentication')) {
    return NextResponse.json({ success: false, error: error.message }, { status: 401 })
  }

  if (error instanceof Error && error.message.includes('not found')) {
    return NextResponse.json({ success: false, error: error.message }, { status: 404 })
  }

  if (error instanceof Error && error.message.includes('denied')) {
    return NextResponse.json({ success: false, error: error.message }, { status: 403 })
  }

  return NextResponse.json(
    { success: false, error: 'Internal server error' },
    { status: 500 },
  )
}
