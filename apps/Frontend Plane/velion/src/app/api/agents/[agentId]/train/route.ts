import { NextRequest, NextResponse } from 'next/server'
import { z } from 'zod'

import { resolveChatActor, ChatStoreError } from '@/app/api/chat/_lib/session-store'
import { getModelPlaneTokenFromSession } from '@/lib/model-plane/auth-token'
import { getKnowledgeDocuments, getKnowledgeDocumentDetail } from '@/app/api/knowledge/_lib/knowledge-data'

const MODEL_GATEWAY_URL =
  process.env.MODEL_GATEWAY_URL ?? 'http://model-plane-model-gateway-1:8080'

const trainSchema = z.object({
  /**
   * When true, use only docs the AI router (Phase 7) classified as
   * `route='finetune'`. When false, use the explicit list passed in
   * `documentIds` (operator manual selection from the Train matrix).
   */
  aiSelect: z.boolean(),
  documentIds: z.array(z.string()).optional(),
  baseModel: z.string().min(1).max(128).default('gpt-4o-mini'),
  hyperparameters: z
    .object({
      epochs: z.number().int().min(1).max(20).optional(),
      learningRateMultiplier: z.number().min(0.01).max(10).optional(),
    })
    .optional(),
})

interface RouteContext {
  params: Promise<{ agentId: string }>
}

interface RoutableDoc {
  id: string
  title: string
  type: string
  content?: string
  routing?: { route?: string; routedBy?: string }
}

/**
 * Wave 11 §8 — kick a fine-tune run from the agent's Train tab.
 *
 * Composes JSONL training examples from the AI-routed `finetune` slice
 * (or operator's manual selection), forwards as multipart to the
 * existing Wave 7 fine-tune endpoint. The Train tab polls the existing
 * `GET /api/agents/{agentId}/finetune` for status — we don't reinvent
 * the polling here.
 */
export async function POST(request: NextRequest, context: RouteContext) {
  try {
    const { agentId } = await context.params
    const actor = await resolveChatActor()
    const body = await request.json()
    const parsed = trainSchema.parse(body)

    const all = await getKnowledgeDocuments({ limit: 500 })

    let trainingDocs: RoutableDoc[] = []
    if (parsed.aiSelect) {
      // Hydrate routing field for each doc — getKnowledgeDocuments may
      // not include it, so we fetch detail on the candidate set.
      const candidates = all.documents
      const enriched = await Promise.all(
        candidates.map(async (doc) => {
          const detail = await getKnowledgeDocumentDetail(doc.id).catch(() => null)
          const routing = (detail as unknown as { routing?: RoutableDoc['routing'] })?.routing
          return { id: doc.id, title: doc.title, type: doc.type, content: detail?.content, routing }
        }),
      )
      trainingDocs = enriched.filter((doc) => doc.routing?.route === 'finetune')
    } else {
      const wanted = new Set(parsed.documentIds ?? [])
      const candidates = all.documents.filter((doc) => wanted.has(doc.id))
      trainingDocs = await Promise.all(
        candidates.map(async (doc) => {
          const detail = await getKnowledgeDocumentDetail(doc.id).catch(() => null)
          return {
            id: doc.id,
            title: doc.title,
            type: doc.type,
            content: detail?.content,
          }
        }),
      )
    }

    if (trainingDocs.length === 0) {
      return NextResponse.json(
        {
          error:
            'No fine-tune training data. Auto-route your knowledge first, or pick documents manually.',
        },
        { status: 400 },
      )
    }

    // Compose JSONL — chat-completion format. Each doc becomes one
    // example: the title becomes a paraphrased user prompt, the body
    // becomes the assistant response. The fine-tune training run
    // teaches the model to *imitate* the doc style/voice, which is the
    // whole point of routing a doc to `finetune` (vs RAG retrieval).
    const lines = trainingDocs
      .filter((doc) => Boolean(doc.content && doc.content.trim()))
      .map((doc) => {
        const system =
          'You are an assistant trained on this organization\'s reference style and tone.'
        return JSON.stringify({
          messages: [
            { role: 'system', content: system },
            {
              role: 'user',
              content: doc.title || 'Please respond in our reference style.',
            },
            { role: 'assistant', content: doc.content!.trim().slice(0, 8000) },
          ],
        })
      })

    if (lines.length === 0) {
      return NextResponse.json(
        { error: 'No fine-tune docs have extractable content yet — try again after indexing finishes.' },
        { status: 400 },
      )
    }

    // Build multipart body and forward to the existing fine-tune route.
    const jsonl = lines.join('\n')
    const jsonlBytes = new TextEncoder().encode(jsonl)
    const form = new FormData()
    form.append('file', new Blob([jsonlBytes], { type: 'application/jsonl' }), 'training.jsonl')
    form.append('base_model', parsed.baseModel)
    if (parsed.hyperparameters) {
      form.append('hyperparameters', JSON.stringify(parsed.hyperparameters))
    }

    const token = await getModelPlaneTokenFromSession(request)
    const url = new URL(`${MODEL_GATEWAY_URL}/v1/finetune/jobs`)
    url.searchParams.set('agent_id', agentId)
    url.searchParams.set('org_id', actor.orgId)

    const upstream = await fetch(url.toString(), {
      method: 'POST',
      headers: { Authorization: `Bearer ${token}` },
      body: form,
      signal: AbortSignal.timeout(60_000),
    })

    const text = await upstream.text()
    if (!upstream.ok) {
      return NextResponse.json(
        { error: text || `Gateway returned ${upstream.status}` },
        { status: upstream.status },
      )
    }

    let parsedBody: unknown = null
    try {
      parsedBody = JSON.parse(text)
    } catch {
      parsedBody = { raw: text }
    }

    return NextResponse.json(
      {
        success: true,
        jobsCount: lines.length,
        result: parsedBody,
      },
      { status: 202 },
    )
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
      { error: error instanceof Error ? error.message : 'Training kick failed' },
      { status: 500 },
    )
  }
}
