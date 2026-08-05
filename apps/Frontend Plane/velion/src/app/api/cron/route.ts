import { NextRequest, NextResponse } from 'next/server'

import { getModelPlaneTokenFromSession } from '@/lib/model-plane/auth-token'

/**
 * U3-12 (ui-ux-verevon-gap.md §14): verevon proxy for the gateway's
 * `/v1/cron` surface (capability-core via model-gateway). Lists or
 * creates org-scoped cron entries; the gateway injects the caller's
 * org_id from the JWT before forwarding to capability-core.
 *
 * Per-agent filtering is done client-side via the `agent_id` field on
 * each cron row — capability-core stores it but doesn't index by it
 * today. As cron lists stay small (typically <100 per org), the client
 * filter is fine; revisit if a single org passes ~1k cron entries.
 *
 * Request shapes match the gateway's pass-through to capability-core:
 *   POST { agent_id, schedule, payload, ... }
 *   GET  → { items: [...] }
 */

const MODEL_GATEWAY_URL =
  process.env.MODEL_GATEWAY_URL ?? 'http://model-plane-model-gateway-1:8080'

export async function GET(request: NextRequest): Promise<Response> {
  try {
    const token = await getModelPlaneTokenFromSession(request)
    const upstream = await fetch(`${MODEL_GATEWAY_URL}/v1/cron`, {
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
      { error: 'cron list unavailable', message: e instanceof Error ? e.message : 'unknown' },
      { status: 502 },
    )
  }
}

export async function POST(request: NextRequest): Promise<Response> {
  try {
    const body = await request.json()
    const token = await getModelPlaneTokenFromSession(request)
    const upstream = await fetch(`${MODEL_GATEWAY_URL}/v1/cron`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        Authorization: `Bearer ${token}`,
      },
      body: JSON.stringify(body),
      signal: AbortSignal.timeout(15_000),
    })
    const text = await upstream.text()
    return new NextResponse(text, {
      status: upstream.status,
      headers: { 'Content-Type': 'application/json' },
    })
  } catch (e: unknown) {
    return NextResponse.json(
      { error: 'cron create failed', message: e instanceof Error ? e.message : 'unknown' },
      { status: 502 },
    )
  }
}
