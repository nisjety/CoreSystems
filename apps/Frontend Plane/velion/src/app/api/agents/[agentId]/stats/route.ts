import { NextRequest, NextResponse } from 'next/server'

import { resolveChatActor } from '@/app/api/chat/_lib/session-store'
import { convexQuery } from '@/app/api/_lib/convex-client'

/**
 * U3-9 (ui-ux-verevon-gap.md §14): per-agent run statistics.
 *
 * Reads the Convex `agentRuns` mirror (populated by `convex-subscriber` from
 * orchestrator-core's `mp.v1.run.*.event` NATS subjects per U3-3 / W4-2) and
 * returns honest aggregate metrics for the AgentWorkspaceView analytics tab.
 *
 * Response shape:
 *   {
 *     total: number,
 *     started: number, completed: number, failed: number, cancelled: number,
 *     successRate: number,      // 0..1
 *     avgDurationMs: number,
 *     lookbackDays: number,
 *     recent: Array<{ runId, status, startedAt, completedAt, error }>
 *   }
 *
 * The query falls back to zero counts when the org has no run history —
 * the UI renders this as "No runs yet" rather than fake numbers.
 */

interface AgentRunStats {
  total: number
  started: number
  completed: number
  failed: number
  cancelled: number
  successRate: number
  avgDurationMs: number
  lookbackDays: number
  recent: Array<{
    runId: string
    status: string
    startedAt: number
    completedAt: number | null
    error: string | null
  }>
}

const EMPTY_STATS: AgentRunStats = {
  total: 0,
  started: 0,
  completed: 0,
  failed: 0,
  cancelled: 0,
  successRate: 0,
  avgDurationMs: 0,
  lookbackDays: 30,
  recent: [],
}

export async function GET(
  request: NextRequest,
  { params }: { params: Promise<{ agentId: string }> },
): Promise<Response> {
  const { agentId } = await params
  const lookbackParam = request.nextUrl.searchParams.get('days')
  const lookbackDays = lookbackParam ? Number(lookbackParam) : 30

  try {
    const actor = await resolveChatActor()
    const stats = await convexQuery<AgentRunStats | null>(
      'agentRuns:statsByAgent',
      {
        externalOrgId: actor.convexOrgId,
        agentId,
        lookbackDays: Number.isFinite(lookbackDays) ? lookbackDays : 30,
      },
    )
    return NextResponse.json(stats ?? EMPTY_STATS)
  } catch (error: unknown) {
    return NextResponse.json(
      {
        ...EMPTY_STATS,
        error: error instanceof Error ? error.message : 'stats unavailable',
      },
      { status: 200 },
    )
  }
}
