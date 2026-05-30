import { NextRequest, NextResponse } from 'next/server'

import { resolveChatActor, ChatStoreError } from '@/app/api/chat/_lib/session-store'

const AGENT_CORE_URL = process.env.AGENT_CORE_URL || 'http://localhost:8002'
const INTERNAL_API_KEY = process.env.INTERNAL_API_KEY ?? process.env.INTERNAL_SERVICE_SECRET ?? ''

function agentCoreHeaders(orgId: string): Record<string, string> {
  const h: Record<string, string> = {
    'Content-Type': 'application/json',
    'X-Org-Id': orgId,
  }
  if (INTERNAL_API_KEY) h['X-Internal-Api-Key'] = INTERNAL_API_KEY
  return h
}

export async function POST(
  _request: NextRequest,
  { params }: { params: Promise<{ id: string }> },
) {
  try {
    const actor = await resolveChatActor()
    const { id } = await params

    const res = await fetch(`${AGENT_CORE_URL}/v1/mcp/servers/${id}/test`, {
      method: 'POST',
      headers: agentCoreHeaders(actor.convexOrgId),
      signal: AbortSignal.timeout(15_000),
    })
    const data = await res.json()
    if (!res.ok) {
      return NextResponse.json({ error: data?.detail ?? 'Test failed' }, { status: res.status })
    }
    return NextResponse.json(data)
  } catch (error) {
    if (error instanceof ChatStoreError) {
      return NextResponse.json({ error: error.message }, { status: error.statusCode })
    }
    return NextResponse.json({ error: 'Internal server error' }, { status: 500 })
  }
}
