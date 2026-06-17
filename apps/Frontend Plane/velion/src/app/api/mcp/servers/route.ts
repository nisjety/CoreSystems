import { NextRequest, NextResponse } from 'next/server'
import { z } from 'zod'

import { resolveChatActor, ChatStoreError } from '@/app/api/chat/_lib/session-store'

const AGENT_CORE_URL = process.env.AGENT_CORE_URL || 'http://localhost:8002'
const INTERNAL_API_KEY = process.env.INTERNAL_API_KEY ?? process.env.INTERNAL_SERVICE_SECRET ?? ''

const createServerSchema = z.object({
  name: z.string().min(1).max(128),
  transport: z.enum(['stdio', 'http', 'sse', 'ws']),
  command: z.string().optional(),
  args: z.array(z.string()).optional(),
  env: z.record(z.string(), z.string()).optional(),
  url: z.string().url().optional().or(z.literal('')),
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

export async function GET() {
  try {
    const actor = await resolveChatActor()
    const res = await fetch(`${AGENT_CORE_URL}/v1/mcp/servers`, {
      headers: agentCoreHeaders(actor.convexOrgId),
      signal: AbortSignal.timeout(8_000),
    })
    if (!res.ok) {
      const detail = await res.text()
      return NextResponse.json({ error: detail || 'agent-core error' }, { status: res.status })
    }
    const data = await res.json()
    return NextResponse.json(data)
  } catch (error) {
    if (error instanceof ChatStoreError) {
      return NextResponse.json({ error: error.message }, { status: error.statusCode })
    }
    return NextResponse.json({ error: 'Internal server error' }, { status: 500 })
  }
}

export async function POST(request: NextRequest) {
  try {
    const actor = await resolveChatActor()
    const body = await request.json()
    const parsed = createServerSchema.parse(body)

    const res = await fetch(`${AGENT_CORE_URL}/v1/mcp/servers`, {
      method: 'POST',
      headers: agentCoreHeaders(actor.convexOrgId),
      body: JSON.stringify(parsed),
      signal: AbortSignal.timeout(8_000),
    })
    const data = await res.json()
    if (!res.ok) {
      return NextResponse.json({ error: data?.detail ?? 'Failed to create server' }, { status: res.status })
    }
    return NextResponse.json(data, { status: 201 })
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
