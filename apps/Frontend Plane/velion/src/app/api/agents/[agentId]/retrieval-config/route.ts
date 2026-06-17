import { NextRequest, NextResponse } from 'next/server'
import { z } from 'zod'

import { resolveChatActor, ChatStoreError } from '@/app/api/chat/_lib/session-store'
import { convexMutation } from '@/app/api/_lib/convex-client'

const weightsSchema = z.object({
  dense: z.number().min(0).max(1),
  bm25: z.number().min(0).max(1),
  graph: z.number().min(0).max(1),
  wiki: z.number().min(0).max(1),
})

const configSchema = z.object({
  weights: weightsSchema.optional(),
  chunkSize: z.number().int().min(64).max(8192).optional(),
  topK: z.number().int().min(1).max(50).optional(),
  rerank: z.boolean().optional(),
  graphHops: z.number().int().min(0).max(3).optional(),
})

interface RouteContext {
  params: Promise<{ agentId: string }>
}

/**
 * Wave 11 §6.5 — Configure RAG (ElevenLabs).
 *
 * Operator-set knobs only — server defaults remain authoritative when a
 * field is absent. The model-gateway forwards the persisted shape to
 * `retrieval-engine-rs` on every hybrid retrieval call.
 */
export async function PUT(request: NextRequest, context: RouteContext) {
  try {
    const { agentId } = await context.params
    const actor = await resolveChatActor()
    const body = await request.json()
    const parsed = configSchema.parse(body)

    // Weights must roughly sum to 1.0 (±0.01) to avoid silent scoring drift.
    if (parsed.weights) {
      const sum =
        parsed.weights.dense + parsed.weights.bm25 + parsed.weights.graph + parsed.weights.wiki
      if (Math.abs(sum - 1) > 0.01) {
        return NextResponse.json(
          { error: `Retrieval weights must sum to 1.0 (got ${sum.toFixed(2)})` },
          { status: 400 },
        )
      }
    }

    const updated = await convexMutation<unknown>('agents:updateRetrievalConfig', {
      agentId,
      orgId: actor.convexOrgId,
      ...parsed,
    })

    return NextResponse.json({ success: true, agent: updated })
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
      { error: error instanceof Error ? error.message : 'Update failed' },
      { status: 500 },
    )
  }
}
