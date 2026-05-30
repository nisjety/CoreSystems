import { NextRequest, NextResponse } from 'next/server'
import { ZodError, z } from 'zod'

import { resolveChatActor } from '@/app/api/chat/_lib/session-store'
import { convexMutation, convexQuery, CONVEX_INTERNAL_SERVICE_KEY } from '@/app/api/_lib/convex-client'
import type { PersistedAgent } from '@/components/agents/types'

const createAgentSchema = z.object({
  name: z.string().trim().min(1).max(128),
  description: z.string().max(500).optional(),
  useCase: z.enum([
    'customer_support', 'sales', 'marketing', 'hr', 'faq', 'onboarding', 'other',
  ]),
  status: z.enum(['active', 'inactive', 'draft']).default('draft'),
  profile: z.enum(['chat', 'deployed_agent']).optional(),
  // U2-1 follow-up: aligned with the real Azure deployment names
  // (`gpt-5.4-mini` was fictional). See chat/_lib/models.ts.
  model: z.string().trim().min(1).max(128).default('gpt-4o-mini'),
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

export async function GET(_request: NextRequest) {
  try {
    const actor = await resolveChatActor()
    const agents = await convexQuery<PersistedAgent[]>('agents:listByOrg', {
      orgId: actor.convexOrgId,
    })
    return NextResponse.json({ success: true, data: agents })
  } catch (error) {
    return errorResponse(error)
  }
}

export async function POST(request: NextRequest) {
  try {
    const actor = await resolveChatActor()
    const body = await request.json()
    const parsed = createAgentSchema.parse(body)

    const agentId = await convexMutation<string>('agents:create', {
      orgId: actor.convexOrgId,
      createdBy: actor.convexUserId,
      ...parsed,
    })

    const agent = await convexQuery<PersistedAgent | null>('agents:getById', {
      agentId,
      orgId: actor.convexOrgId,
    })

    return NextResponse.json({ success: true, data: agent }, { status: 201 })
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

  return NextResponse.json(
    { success: false, error: 'Internal server error' },
    { status: 500 },
  )
}
