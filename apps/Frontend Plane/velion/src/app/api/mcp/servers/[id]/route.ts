import { NextRequest, NextResponse } from 'next/server'
import { z } from 'zod'

import { resolveChatActor, ChatStoreError } from '@/app/api/chat/_lib/session-store'

const AGENT_CORE_URL = process.env.AGENT_CORE_URL || 'http://localhost:8002'
const INTERNAL_API_KEY = process.env.INTERNAL_API_KEY ?? process.env.INTERNAL_SERVICE_SECRET ?? ''

const updateServerSchema = z.object({
  name: z.string().min(1).max(128).optional(),
  transport: z.enum(['stdio', 'http', 'sse', 'ws']).optional(),
  command: z.string().optional(),
  args: z.array(z.string()).optional(),
  env: z.record(z.string(), z.string()).optional(),
  url: z.string().optional(),
  headers: z.record(z.string(), z.string()).optional(),
  enabled: z.boolean().optional(),
})

function agentCoreHeaders(orgId: string): Record<string, string> {
  const h: Record<string, string> = {
    'Content-Type': 'application/json',
    'X-Org-Id': orgId,
  }
  if (INTERNAL_API_KEY) h['X-Internal-Api-Key'] = INTERNAL_API_KEY
  return h
}

export async function PATCH(
  request: NextRequest,
  { params }: { params: Promise<{ id: string }> },
) {
  try {
    const actor = await resolveChatActor()
    const { id } = await params
    const body = await request.json()
    const parsed = updateServerSchema.parse(body)

    const res = await fetch(`${AGENT_CORE_URL}/v1/mcp/servers/${id}`, {
      method: 'PATCH',
      headers: agentCoreHeaders(actor.convexOrgId),
      body: JSON.stringify(parsed),
      signal: AbortSignal.timeout(8_000),
    })
    if (res.status === 204) return new NextResponse(null, { status: 204 })
    const data = await res.json()
    if (!res.ok) {
      return NextResponse.json({ error: data?.detail ?? 'Failed to update server' }, { status: res.status })
    }
    return NextResponse.json(data)
  } catch (error) {
    if (error instanceof ChatStoreError) {
      return NextResponse.json({ error: error.message }, { status: error.statusCode })
    }
    if (error instanceof z.ZodError) {
      return NextResponse.json({ error: error.issues[0]?.message ?? 'Validation failed' }, { status: 400 })
    }
    return NextResponse.json({ error: 'Internal server error' }, { status: 500 })
  }
}

export async function DELETE(
  _request: NextRequest,
  { params }: { params: Promise<{ id: string }> },
) {
  try {
    const actor = await resolveChatActor()
    const { id } = await params

    const res = await fetch(`${AGENT_CORE_URL}/v1/mcp/servers/${id}`, {
      method: 'DELETE',
      headers: agentCoreHeaders(actor.convexOrgId),
      signal: AbortSignal.timeout(8_000),
    })
    if (!res.ok) {
      const detail = await res.text()
      return NextResponse.json({ error: detail || 'Failed to delete server' }, { status: res.status })
    }
    return new NextResponse(null, { status: 204 })
  } catch (error) {
    if (error instanceof ChatStoreError) {
      return NextResponse.json({ error: error.message }, { status: error.statusCode })
    }
    return NextResponse.json({ error: 'Internal server error' }, { status: 500 })
  }
}
