import { NextRequest, NextResponse } from 'next/server'

import { getModelPlaneTokenFromSession } from '@/lib/model-plane/auth-token'

/**
 * Wave 7 (verevon ui-ux-verevon-gap.md §17): per-agent fine-tuning proxy.
 *
 * GET  /api/agents/{agentId}/finetune
 *   Lists past jobs for this agent (scoped to org via JWT).
 *
 * POST /api/agents/{agentId}/finetune
 *   Multipart upload (`file` = JSONL, plus `base_model`, optional
 *   `hyperparameters`). Forwards to model-gateway
 *   `POST /v1/finetune/jobs`. The gateway requires the `admin` scope
 *   on the JWT — auth-core's `issueModelPlaneToken` adds it only for
 *   org owners / admins (see `apps/Control Plane/auth-core/...`).
 *   Non-admins get 403 with a clear message.
 *
 * Cancellation lives at `./[jobId]/route.ts`.
 */

const MODEL_GATEWAY_URL =
  process.env.MODEL_GATEWAY_URL ?? 'http://model-plane-model-gateway-1:8080'

export async function GET(
  request: NextRequest,
  { params }: { params: Promise<{ agentId: string }> },
): Promise<Response> {
  try {
    const { agentId } = await params
    const token = await getModelPlaneTokenFromSession(request)
    const url = new URL(`${MODEL_GATEWAY_URL}/v1/finetune/jobs`)
    url.searchParams.set('agent_id', agentId)

    const upstream = await fetch(url.toString(), {
      method: 'GET',
      headers: { Authorization: `Bearer ${token}` },
      signal: AbortSignal.timeout(15_000),
    })
    const text = await upstream.text()
    return new NextResponse(text, {
      status: upstream.status,
      headers: { 'Content-Type': 'application/json' },
    })
  } catch (e: unknown) {
    return NextResponse.json(
      {
        error: 'finetune list failed',
        message: e instanceof Error ? e.message : 'unknown',
      },
      { status: 502 },
    )
  }
}

export async function POST(
  request: NextRequest,
  { params }: { params: Promise<{ agentId: string }> },
): Promise<Response> {
  try {
    const { agentId } = await params
    const token = await getModelPlaneTokenFromSession(request)

    // Re-stream the incoming multipart body through to the gateway.
    // We can't just forward `request.body` because we need to
    // **append** the `agent_id` field (the route URL carries it for
    // the verevon proxy, but the gateway expects it inside the body).
    const incoming = await request.formData()
    const forwarded = new FormData()
    for (const [key, value] of incoming.entries()) {
      // Drop any client-supplied agent_id so the URL-derived id wins.
      if (key === 'agent_id') continue
      // Re-add as Blob so File handling stays correct.
      forwarded.append(key, value as Blob | string)
    }
    forwarded.set('agent_id', agentId)

    const upstream = await fetch(`${MODEL_GATEWAY_URL}/v1/finetune/jobs`, {
      method: 'POST',
      headers: { Authorization: `Bearer ${token}` },
      body: forwarded,
      // Fine-tune file uploads + Azure kickoff can take 30s+; allow
      // generous timeout. The actual training is async — we only wait
      // for the kickoff acknowledgement.
      signal: AbortSignal.timeout(60_000),
    })
    const text = await upstream.text()
    return new NextResponse(text, {
      status: upstream.status,
      headers: { 'Content-Type': 'application/json' },
    })
  } catch (e: unknown) {
    return NextResponse.json(
      {
        error: 'finetune kickoff failed',
        message: e instanceof Error ? e.message : 'unknown',
      },
      { status: 502 },
    )
  }
}
