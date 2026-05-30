import { NextRequest, NextResponse } from 'next/server'
import { z } from 'zod'

import { resolveChatActor, ChatStoreError } from '@/app/api/chat/_lib/session-store'
import { convexMutation, convexQuery } from '@/app/api/_lib/convex-client'
import { getCorrelationId } from '@/app/api/_lib/control-plane-auth'
import { harnessJson } from '@/lib/model-plane/harness-client'

const rateSchema = z.object({
  rating: z.enum(['good', 'acceptable', 'poor']),
  note: z.string().max(500).optional(),
})

interface RouteContext {
  params: Promise<{ runId: string }>
}

/**
 * Wave 11 §5 — Fin G/A/P feedback persist.
 *
 * The playground reply chips POST here; Convex mutation `agentRuns.rate`
 * stores `rating` + `ratedAt` + optional note on the existing
 * `agentRuns` row. Re-rating overwrites. Org-scoping happens both here
 * (via `resolveChatActor`) and inside the mutation as belt-and-braces.
 */
export async function POST(request: NextRequest, context: RouteContext) {
  try {
    const { runId } = await context.params
    const actor = await resolveChatActor()
    const body = await request.json()
    const parsed = rateSchema.parse(body)

    const updated = await convexMutation<unknown>('agentRuns:rate', {
      runId,
      externalOrgId: actor.orgId,
      rating: parsed.rating,
      note: parsed.note,
    })

    // Emit the promotion signal (best-effort) so orchestrator-core's
    // FeedbackPromotionWorkflow can fold this rating into skill promotion
    // (HARNESS_PHASE1 §6). The agent acts as the skill being rated.
    try {
      const run = await convexQuery<{ agentId?: string } | null>('agentRuns:getByRunId', {
        runId,
      })
      if (run?.agentId) {
        await harnessJson({
          path: '/v1/feedback',
          method: 'POST',
          body: { run_id: runId, skill_id: run.agentId, rating: parsed.rating },
          auth: { cookieHeader: request.headers.get('cookie') ?? '' },
          correlationId: getCorrelationId(request),
          retry: false,
        })
      }
    } catch {
      // Signal emission is best-effort; the durable rating is already persisted.
    }

    return NextResponse.json({ success: true, run: updated }, { status: 200 })
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
      { error: error instanceof Error ? error.message : 'Rate failed' },
      { status: 500 },
    )
  }
}
