import { NextRequest, NextResponse } from 'next/server'

import { getModelPlaneTokenFromSession } from '@/lib/model-plane/auth-token'

/**
 * Wave 7 (velion ui-ux-velion-gap.md §17): per-job finetune proxy.
 *
 * GET    /api/agents/{agentId}/finetune/{jobId}  — single-row read
 * DELETE /api/agents/{agentId}/finetune/{jobId}  — cancel (admin scope
 *                                                  required by the gateway)
 *
 * The agentId in the URL is informational only — capability-core
 * scopes by job id; the org scope is enforced via JWT.
 */

const MODEL_GATEWAY_URL =
  process.env.MODEL_GATEWAY_URL ?? 'http://model-plane-model-gateway-1:8080'

async function forward(
  request: NextRequest,
  jobId: string,
  method: 'GET' | 'DELETE',
): Promise<Response> {
  try {
    const token = await getModelPlaneTokenFromSession(request)
    const upstream = await fetch(
      `${MODEL_GATEWAY_URL}/v1/finetune/jobs/${encodeURIComponent(jobId)}`,
      {
        method,
        headers: { Authorization: `Bearer ${token}` },
        signal: AbortSignal.timeout(20_000),
      },
    )
    const text = await upstream.text()
    return new NextResponse(text || '{}', {
      status: upstream.status,
      headers: { 'Content-Type': 'application/json' },
    })
  } catch (e: unknown) {
    return NextResponse.json(
      {
        error: `finetune ${method.toLowerCase()} failed`,
        message: e instanceof Error ? e.message : 'unknown',
      },
      { status: 502 },
    )
  }
}

export async function GET(
  request: NextRequest,
  { params }: { params: Promise<{ agentId: string; jobId: string }> },
): Promise<Response> {
  const { jobId } = await params
  return forward(request, jobId, 'GET')
}

export async function DELETE(
  request: NextRequest,
  { params }: { params: Promise<{ agentId: string; jobId: string }> },
): Promise<Response> {
  const { jobId } = await params
  return forward(request, jobId, 'DELETE')
}
