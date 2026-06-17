import { NextRequest, NextResponse } from 'next/server'
import { z } from 'zod'

import { convexQuery } from '@/app/api/_lib/convex-client'
import { getModelPlaneTokenInternal } from '@/lib/model-plane/auth-token'
import type { PersistedAgent } from '@/components/agents/types'

/**
 * Wave 9 (ui-ux-velion-gap.md §19): public embed-widget message-send.
 *
 * Anonymous visitor flow — no Better Auth session required. Each
 * conversation is keyed by a `visitor_id` the widget generates
 * client-side (random UUID, persisted to localStorage on the host page).
 *
 * Security model:
 *   - The agent must have `publicEnabled=true` AND the supplied
 *     `secret` must match `publicSecret`. Mismatch / disabled → 404
 *     (same shape as missing, so scraping agent ids is fruitless).
 *   - We mint a SERVICE-TO-SERVICE Model Plane JWT (admin scope
 *     intentionally omitted) using the agent's `orgId` as the JWT
 *     `org_id`. The gateway sees the call as an org-internal request
 *     against the agent's configured model + tools.
 *   - No JWT minted for the visitor — they don't authenticate. The
 *     boundary of trust is the publicSecret + the agent's enabled flag.
 *
 * Response: SSE stream of `data: {type, content}\n\n` frames, same
 * shape as the authenticated chat path so the public widget's renderer
 * can reuse the parsing logic.
 *
 * Rate-limit: piggybacks on the gateway's per-org rate limiter. A
 * dedicated per-visitor limit lives on a Wave 9.1 follow-up.
 */

const EmbedRequestSchema = z.object({
  message: z.string().trim().min(1).max(4000),
  secret: z.string().trim().min(1),
  visitorId: z.string().trim().min(1).max(128),
})

interface EmbedConfig {
  id: string
  name: string
  greeting: string
  theme: Record<string, unknown>
  orgId: string
}

const MODEL_GATEWAY_URL =
  process.env.MODEL_GATEWAY_URL ?? 'http://model-plane-model-gateway-1:8080'

export async function POST(
  request: NextRequest,
  { params }: { params: Promise<{ agentId: string }> },
): Promise<Response> {
  const { agentId } = await params

  let body: z.infer<typeof EmbedRequestSchema>
  try {
    body = EmbedRequestSchema.parse(await request.json())
  } catch {
    return NextResponse.json(
      { error: 'invalid_request' },
      {
        status: 400,
        headers: { 'Access-Control-Allow-Origin': '*' },
      },
    )
  }

  // Verify the agent is publicly enabled and the secret matches.
  let config: EmbedConfig | null = null
  try {
    config = await convexQuery<EmbedConfig | null>('agents:getEmbedConfig', {
      agentId,
      publicSecret: body.secret,
    })
  } catch {
    config = null
  }
  if (!config) {
    return NextResponse.json(
      { error: 'not_found' },
      {
        status: 404,
        headers: { 'Access-Control-Allow-Origin': '*' },
      },
    )
  }

  // Load the agent's full config (system prompt, model, tools) — needs
  // the same Convex query the auth'd path uses, but we have to thread
  // the orgId from the embed config rather than from a session.
  let agentConfig: PersistedAgent | null = null
  try {
    agentConfig = await convexQuery<PersistedAgent | null>('agents:getById', {
      agentId,
      orgId: config.orgId,
    })
  } catch {
    /* not fatal — we still have the public config */
  }

  // Mint an internal-key bearer scoped to the agent's org. No admin
  // scope (the visitor isn't an admin); fine-tune/cron routes would
  // reject this token, which is exactly what we want.
  let token: string
  try {
    token = await getModelPlaneTokenInternal({
      userId: `embed-visitor:${body.visitorId}`,
      orgId: config.orgId,
    })
  } catch (e: unknown) {
    return NextResponse.json(
      {
        error: 'embed_unavailable',
        detail: e instanceof Error ? e.message : 'unknown',
      },
      { status: 502, headers: { 'Access-Control-Allow-Origin': '*' } },
    )
  }

  // Build the gateway POST body — mirror the authenticated path so the
  // gateway's tool loop, browse_web grounding, system prompt prefix
  // all behave identically.
  const gatewayBody = {
    content: body.message,
    model: agentConfig?.model,
    session_key: `embed:${agentId}:${body.visitorId}`,
    response_mode: 'auto' as const,
    browse_web: true,
    system_prompt: agentConfig?.systemPrompt,
    tools: agentConfig?.tools,
  }

  // The gateway returns a unary JSON envelope; we re-stream it as SSE
  // so the embed widget can show a "typing…" indicator while waiting.
  let upstream: Response
  try {
    upstream = await fetch(`${MODEL_GATEWAY_URL}/v1/invoke`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        Authorization: `Bearer ${token}`,
      },
      body: JSON.stringify(gatewayBody),
      signal: AbortSignal.timeout(90_000),
    })
  } catch (e: unknown) {
    return NextResponse.json(
      {
        error: 'gateway_unavailable',
        detail: e instanceof Error ? e.message : 'unknown',
      },
      { status: 502, headers: { 'Access-Control-Allow-Origin': '*' } },
    )
  }

  if (!upstream.ok) {
    return NextResponse.json(
      { error: 'gateway_error', upstream_status: upstream.status },
      {
        status: 502,
        headers: { 'Access-Control-Allow-Origin': '*' },
      },
    )
  }

  const envelope = (await upstream.json()) as {
    content?: string
    model_used?: string
  }
  const content = (envelope.content ?? '').trim()

  // SSE response — one chunk + done. Keeps the widget's parser simple
  // and matches the authenticated chat path's shape.
  const encoder = new TextEncoder()
  const stream = new ReadableStream({
    start(controller) {
      const send = (obj: object): void => {
        controller.enqueue(encoder.encode(`data: ${JSON.stringify(obj)}\n\n`))
      }
      send({ type: 'answer_chunk', content })
      send({ type: 'done', model: envelope.model_used })
      controller.close()
    },
  })

  return new Response(stream, {
    headers: {
      'Content-Type': 'text/event-stream',
      'Cache-Control': 'no-cache',
      Connection: 'keep-alive',
      // Permissive CORS — embed lives on arbitrary third-party sites.
      'Access-Control-Allow-Origin': '*',
    },
  })
}

export async function OPTIONS(): Promise<Response> {
  return new NextResponse(null, {
    status: 204,
    headers: {
      'Access-Control-Allow-Origin': '*',
      'Access-Control-Allow-Methods': 'POST, OPTIONS',
      'Access-Control-Allow-Headers': 'Content-Type',
      'Access-Control-Max-Age': '86400',
    },
  })
}
