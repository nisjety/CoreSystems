import { NextRequest, NextResponse } from 'next/server'

import {
  authErrorResponse,
  getCorrelationId,
  requireSession,
} from '@/app/api/_lib/control-plane-auth'
import { harnessFetch } from '@/lib/model-plane/harness-client'

/**
 * Resume a chat stream after a reload/disconnect (HARNESS_PHASE1 §3b).
 *
 * Forwards to the gateway's `GET /v1/invoke/resume/{request_id}` with the
 * browser's `Last-Event-Id` so it replays buffered deltas after that cursor
 * plus the terminal `done` event. The client remembers `request_id` + last id
 * (e.g. sessionStorage) from the original `/v1/invoke/stream` turn and calls
 * this on remount. A 404 means the buffer expired — restart the request.
 */

interface RouteContext {
  params: Promise<{ requestId: string }>
}

export const dynamic = 'force-dynamic'

export async function GET(request: NextRequest, context: RouteContext): Promise<Response> {
  try {
    await requireSession(request)
  } catch (error) {
    return authErrorResponse(error)
  }

  const { requestId } = await context.params
  const correlationId = getCorrelationId(request)
  const lastEventId = request.headers.get('last-event-id') ?? ''

  let upstream: Response
  try {
    upstream = await harnessFetch({
      path: `/v1/invoke/resume/${encodeURIComponent(requestId)}`,
      method: 'GET',
      accept: 'text/event-stream',
      timeoutMs: 0,
      retry: false,
      correlationId,
      auth: { cookieHeader: request.headers.get('cookie') ?? '' },
      headers: lastEventId ? { 'Last-Event-Id': lastEventId } : {},
      signal: request.signal,
    })
  } catch {
    return NextResponse.json(
      { error: 'resume_unavailable' },
      { status: 502, headers: { 'X-Correlation-Id': correlationId } },
    )
  }

  if (!upstream.ok || !upstream.body) {
    return NextResponse.json(
      { error: `gateway ${upstream.status}` },
      { status: upstream.status, headers: { 'X-Correlation-Id': correlationId } },
    )
  }

  return new Response(upstream.body, {
    status: 200,
    headers: {
      'Content-Type': 'text/event-stream',
      'Cache-Control': 'no-cache, no-transform',
      Connection: 'keep-alive',
      'X-Accel-Buffering': 'no',
      'X-Correlation-Id': correlationId,
    },
  })
}
