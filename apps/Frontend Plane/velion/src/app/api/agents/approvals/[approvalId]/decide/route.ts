import { NextRequest, NextResponse } from 'next/server'
import { z } from 'zod'

import {
  authErrorResponse,
  getCorrelationId,
  requireSession,
} from '@/app/api/_lib/control-plane-auth'
import { harnessJson } from '@/lib/model-plane/harness-client'

/**
 * Operator approval decision (HITL). Forwards to the gateway's
 * `POST /v1/orchestration/approvals/{id}/decide`, which records the decision
 * and broadcasts the state change so the paused run resumes
 * (HARNESS_PHASE1 §4). Used by the operator inbox approve/reject controls.
 */

interface RouteContext {
  params: Promise<{ approvalId: string }>
}

const decideSchema = z.object({
  decision: z.enum(['approve', 'reject']),
  reason: z.string().max(500).optional(),
})

export async function POST(request: NextRequest, context: RouteContext): Promise<Response> {
  try {
    await requireSession(request)
  } catch (error) {
    return authErrorResponse(error)
  }

  const { approvalId } = await context.params
  const correlationId = getCorrelationId(request)

  let body: z.infer<typeof decideSchema>
  try {
    body = decideSchema.parse(await request.json())
  } catch {
    return NextResponse.json({ error: 'invalid_request' }, { status: 400 })
  }

  try {
    const result = await harnessJson<{ approval?: unknown }>({
      path: `/v1/orchestration/approvals/${encodeURIComponent(approvalId)}/decide`,
      method: 'POST',
      body: { decision: body.decision, reason: body.reason ?? '' },
      auth: { cookieHeader: request.headers.get('cookie') ?? '' },
      correlationId,
    })
    return NextResponse.json(
      { success: true, approval: result.approval },
      { headers: { 'X-Correlation-Id': correlationId } },
    )
  } catch {
    return NextResponse.json(
      { error: 'decide_failed' },
      { status: 502, headers: { 'X-Correlation-Id': correlationId } },
    )
  }
}
