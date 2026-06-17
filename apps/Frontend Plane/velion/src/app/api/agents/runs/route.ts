import { NextRequest, NextResponse } from 'next/server'

import { resolveChatActor, ChatStoreError } from '@/app/api/chat/_lib/session-store'
import { convexQuery } from '@/app/api/_lib/convex-client'

/**
 * Operator inbox feed — lists recent agent runs for the org (newest first),
 * optionally filtered by agent and status. Backs the deployed-agent inbox UI.
 *
 * Each run mirrors the Model Plane's lifecycle (started/completed/failed/
 * cancelled) plus the operator's rating. Org-scoped via the chat actor.
 */

interface AgentRun {
  runId: string
  agentId?: string
  status: 'started' | 'completed' | 'failed' | 'cancelled'
  rating?: 'good' | 'acceptable' | 'poor'
  startedAt: number
  completedAt?: number
  error?: string
}

const ALLOWED_STATUS = new Set(['started', 'completed', 'failed', 'cancelled'])

export async function GET(request: NextRequest): Promise<Response> {
  try {
    const actor = await resolveChatActor()
    const url = new URL(request.url)
    const agentId = url.searchParams.get('agentId') ?? undefined
    const statusParam = url.searchParams.get('status') ?? undefined
    const status = statusParam && ALLOWED_STATUS.has(statusParam) ? statusParam : undefined
    const limit = Math.min(Number(url.searchParams.get('limit') ?? 50) || 50, 200)

    const runs = await convexQuery<AgentRun[]>('agentRuns:listForOrg', {
      externalOrgId: actor.orgId,
      limit,
      status,
    })

    // listForOrg is org-scoped; narrow to one agent client-side when asked.
    const filtered = agentId ? runs.filter((r) => r.agentId === agentId) : runs

    return NextResponse.json({ success: true, runs: filtered })
  } catch (error) {
    if (error instanceof ChatStoreError) {
      return NextResponse.json({ error: error.message }, { status: error.statusCode })
    }
    return NextResponse.json({ error: 'runs_unavailable' }, { status: 500 })
  }
}
