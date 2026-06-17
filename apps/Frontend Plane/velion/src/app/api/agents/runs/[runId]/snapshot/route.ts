import { NextRequest, NextResponse } from 'next/server'

import {
  authErrorResponse,
  getCorrelationId,
  requireSession,
} from '@/app/api/_lib/control-plane-auth'
import { harnessJson } from '@/lib/model-plane/harness-client'

/**
 * Run snapshot — the correctness backstop for `useRunEvents` resume
 * (docs/HARNESS_PHASE1.md §3a). The SSE replay buffer is a best-effort fast
 * path; when a client's resume cursor predates the buffer (eviction / restart)
 * it falls back here to re-seed the current authoritative state before tailing
 * live events.
 *
 * Combines the gateway's per-run plan + approval list endpoints into one call.
 * (Todos are thread-scoped and are never delivered through the per-run event
 * feed, so they are intentionally omitted here.)
 */

interface RouteContext {
  params: Promise<{ runId: string }>
}

export const dynamic = 'force-dynamic'

interface PlanValue {
  id: string
  state: string
  summary?: string
}

interface ApprovalValue {
  id: string
  kind: string
  state: string
  decided_by?: string | null
}

export async function GET(request: NextRequest, context: RouteContext): Promise<Response> {
  try {
    await requireSession(request)
  } catch (error) {
    return authErrorResponse(error)
  }

  const { runId } = await context.params
  const correlationId = getCorrelationId(request)
  const auth = { cookieHeader: request.headers.get('cookie') ?? '' }
  const encoded = encodeURIComponent(runId)

  try {
    const [plansRes, approvalsRes] = await Promise.all([
      harnessJson<{ plans?: PlanValue[] }>({
        path: `/v1/orchestration/runs/${encoded}/plans`,
        method: 'GET',
        auth,
        correlationId,
      }),
      harnessJson<{ approvals?: ApprovalValue[] }>({
        path: `/v1/orchestration/runs/${encoded}/approvals`,
        method: 'GET',
        auth,
        correlationId,
      }),
    ])

    return NextResponse.json(
      { plans: plansRes.plans ?? [], approvals: approvalsRes.approvals ?? [] },
      { headers: { 'X-Correlation-Id': correlationId } },
    )
  } catch {
    return NextResponse.json(
      { error: 'snapshot_unavailable', plans: [], approvals: [] },
      { status: 502, headers: { 'X-Correlation-Id': correlationId } },
    )
  }
}
