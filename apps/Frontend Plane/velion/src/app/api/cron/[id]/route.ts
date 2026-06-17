import { NextRequest, NextResponse } from 'next/server'

import { getModelPlaneTokenFromSession } from '@/lib/model-plane/auth-token'

/**
 * U3-12 (ui-ux-velion-gap.md §14): per-cron-entry CRUD proxy.
 * Forwards to model-gateway `/v1/cron/{id}` (capability-core).
 */

const MODEL_GATEWAY_URL =
  process.env.MODEL_GATEWAY_URL ?? 'http://model-plane-model-gateway-1:8080'

async function forward(
  request: NextRequest,
  id: string,
  method: 'GET' | 'PATCH' | 'DELETE',
): Promise<Response> {
  try {
    const token = await getModelPlaneTokenFromSession(request)
    const init: RequestInit = {
      method,
      headers: {
        Authorization: `Bearer ${token}`,
        ...(method === 'PATCH' ? { 'Content-Type': 'application/json' } : {}),
      },
      signal: AbortSignal.timeout(15_000),
    }
    if (method === 'PATCH') {
      init.body = JSON.stringify(await request.json())
    }
    const upstream = await fetch(`${MODEL_GATEWAY_URL}/v1/cron/${encodeURIComponent(id)}`, init)
    const text = await upstream.text()
    return new NextResponse(text || '{}', {
      status: upstream.status,
      headers: { 'Content-Type': 'application/json' },
    })
  } catch (e: unknown) {
    return NextResponse.json(
      { error: `cron ${method.toLowerCase()} failed`, message: e instanceof Error ? e.message : 'unknown' },
      { status: 502 },
    )
  }
}

export async function GET(
  request: NextRequest,
  { params }: { params: Promise<{ id: string }> },
): Promise<Response> {
  const { id } = await params
  return forward(request, id, 'GET')
}

export async function PATCH(
  request: NextRequest,
  { params }: { params: Promise<{ id: string }> },
): Promise<Response> {
  const { id } = await params
  return forward(request, id, 'PATCH')
}

export async function DELETE(
  request: NextRequest,
  { params }: { params: Promise<{ id: string }> },
): Promise<Response> {
  const { id } = await params
  return forward(request, id, 'DELETE')
}
