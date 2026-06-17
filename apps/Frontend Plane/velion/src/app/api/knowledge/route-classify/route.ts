import { NextRequest, NextResponse } from 'next/server'
import { z } from 'zod'

import { resolveChatActor, ChatStoreError } from '@/app/api/chat/_lib/session-store'
import { invokeReasoning } from '@/lib/model-plane/reasoning'
import {
  ROUTER_PROMPT_VERSION,
  ROUTER_SYSTEM_PROMPT,
  buildRouterUserMessage,
} from '@/lib/knowledge/router-prompt'
import {
  parseRouterOutput,
  type RouterRoute,
} from '@/lib/knowledge/router-schema'
import {
  getKnowledgeDocumentDetail,
  updateKnowledgeDocument,
  getKnowledgeDocuments,
} from '@/app/api/knowledge/_lib/knowledge-data'

const requestSchema = z.object({
  documentIds: z.array(z.string()).optional(),
  force: z.boolean().optional(),
})

const THROTTLE_PARALLELISM = 4
const STALE_THRESHOLD_MS = 30 * 24 * 60 * 60 * 1000 // 30 days

interface ClassifyResult {
  documentId: string
  route: RouterRoute
  confidence: number
  reason: string
  routedBy: 'ai' | 'manual'
  routedAt: number
}

/**
 * Wave 11 §7 — AI training-data router.
 *
 * For each requested document (or, if absent, every non-recently-routed
 * doc in the org) we ask model-gateway to classify into one of:
 * rag / graphrag / wiki / finetune / prompt / skip. The result is
 * persisted back to documents-service as a metadata patch.
 *
 *   - Manual overrides (`routedBy='manual'`) are never overwritten
 *     unless `force=true`.
 *   - Auto-routed docs older than 30 days are eligible for re-classification.
 *   - Calls are throttled to 4 parallel to avoid spiking gateway costs.
 */
export async function POST(request: NextRequest) {
  try {
    const actor = await resolveChatActor()
    const body = await request.json().catch(() => ({}))
    const parsed = requestSchema.parse(body)

    // Resolve target set.
    let targetIds: string[]
    if (parsed.documentIds && parsed.documentIds.length > 0) {
      targetIds = parsed.documentIds
    } else {
      const all = await getKnowledgeDocuments({ limit: 500 })
      targetIds = all.documents.map((doc) => doc.id)
    }

    if (targetIds.length === 0) {
      return NextResponse.json({ classified: [], skipped: 0 })
    }

    const now = Date.now()
    const cookieHeader = request.headers.get('cookie') ?? ''

    // Worker that classifies one doc end-to-end.
    const classifyOne = async (docId: string): Promise<ClassifyResult | null> => {
      const detail = await getKnowledgeDocumentDetail(docId)
      const metadata = (detail as unknown as { routing?: { routedBy?: string; routedAt?: number } })
        .routing
      // Respect manual overrides and freshness window.
      if (!parsed.force) {
        if (metadata?.routedBy === 'manual') return null
        if (metadata?.routedAt && now - metadata.routedAt < STALE_THRESHOLD_MS) return null
      }

      const userMessage = buildRouterUserMessage({
        title: detail.title,
        snippet: detail.content ?? '',
        type: detail.type,
      })

      const response = await invokeReasoning(
        {
          query: userMessage,
          strategy: 'fast',
          depth: 'fast',
          context: {
            session_id: `route-classify:${docId}`,
            system_prompt: ROUTER_SYSTEM_PROMPT,
            user_id: actor.userId,
          },
          require_citations: false,
          enable_verification: false,
        },
        { cookieHeader, signal: AbortSignal.timeout(30_000) },
      )
      if (!response.ok) {
        return null
      }
      const envelope = (await response.json()) as { answer?: string }
      const answerText = envelope.answer ?? ''
      const jsonStart = answerText.indexOf('{')
      const jsonEnd = answerText.lastIndexOf('}')
      let rawJson: unknown = null
      if (jsonStart >= 0 && jsonEnd > jsonStart) {
        try {
          rawJson = JSON.parse(answerText.slice(jsonStart, jsonEnd + 1))
        } catch {
          rawJson = null
        }
      }
      const verdict = parseRouterOutput(rawJson)

      // Persist back to documents-service as a metadata patch. We send
      // through `updateKnowledgeDocument` so the service-side embedding
      // worker can react if it cares about the routing column.
      await updateKnowledgeDocument(docId, {
        // We deliberately don't change title/content/status on auto-route.
      })
      // The data-layer helper above is content-only; for routing metadata
      // we hit the service directly. This avoids extending the public
      // patch surface with internal routing fields.
      const documentsServiceUrl =
        process.env.DOCUMENTS_SERVICE_URL ||
        process.env.DOCS_SERVICE_URL ||
        'http://documents-service:8001'
      const internalKey =
        process.env.INTERNAL_API_KEY ||
        process.env.INTERNAL_SERVICE_SECRET ||
        ''
      await fetch(
        `${documentsServiceUrl}/v1/documents/${encodeURIComponent(docId)}/routing`,
        {
          method: 'PATCH',
          headers: {
            'Content-Type': 'application/json',
            'X-Internal-Api-Key': internalKey,
            'X-Org-ID': actor.orgId,
          },
          body: JSON.stringify({
            org_id: actor.orgId,
            route: verdict.route,
            confidence: verdict.confidence,
            reason: verdict.reason,
            routed_by: 'ai',
            routed_at: now,
            prompt_version: ROUTER_PROMPT_VERSION,
          }),
        },
      ).catch(() => null) // documents-service may not yet expose this endpoint locally

      return {
        documentId: docId,
        route: verdict.route,
        confidence: verdict.confidence,
        reason: verdict.reason,
        routedBy: 'ai',
        routedAt: now,
      }
    }

    // Run with bounded parallelism.
    const classified: ClassifyResult[] = []
    let skipped = 0
    for (let i = 0; i < targetIds.length; i += THROTTLE_PARALLELISM) {
      const batch = targetIds.slice(i, i + THROTTLE_PARALLELISM)
      const results = await Promise.allSettled(batch.map(classifyOne))
      for (const result of results) {
        if (result.status === 'fulfilled') {
          if (result.value) classified.push(result.value)
          else skipped += 1
        } else {
          skipped += 1
        }
      }
    }

    return NextResponse.json({ classified, skipped, total: targetIds.length })
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
      { error: error instanceof Error ? error.message : 'Classification failed' },
      { status: 500 },
    )
  }
}
