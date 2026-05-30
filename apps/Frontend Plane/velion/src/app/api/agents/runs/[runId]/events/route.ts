import { NextRequest, NextResponse } from 'next/server'

import {
  authErrorResponse,
  getCorrelationId,
  requireSession,
} from '@/app/api/_lib/control-plane-auth'
import { harnessFetch } from '@/lib/model-plane/harness-client'

/**
 * Operator-facing run-event SSE feed (the shared "task graph" stream).
 *
 * Proxies the model-gateway's `GET /v1/runs/{id}/events` to the browser so a
 * single `useRunEvents(runId)` subscription replaces the per-hook polling that
 * chat / playground / finetune / cron each used to reinvent. The browser's
 * `EventSource` auto-sends `Last-Event-Id` on reconnect; we forward it so
 * session-core replays buffered events after that cursor and then tails live
 * (resume-on-reload, docs/HARNESS_PHASE1.md §3a).
 *
 * Auth: validates the operator's Better Auth session (`requireSession`) before
 * minting a gateway JWT from the same cookie. Anonymous embed widgets use the
 * separate public embed path, not this route.
 */

interface RouteContext {
  params: Promise<{ runId: string }>
}

// SSE must stream — never statically optimized or cached.
export const dynamic = 'force-dynamic'

export async function GET(request: NextRequest, context: RouteContext): Promise<Response> {
  try {
    await requireSession(request)
  } catch (error) {
    return authErrorResponse(error)
  }

  const { runId } = await context.params
  const correlationId = getCorrelationId(request)
  const lastEventId = request.headers.get('last-event-id') ?? ''

  let upstream: Response
  try {
    upstream = await harnessFetch({
      path: `/v1/runs/${encodeURIComponent(runId)}/events`,
      method: 'GET',
      accept: 'text/event-stream',
      // Long-lived stream: no client-side timeout, no retry (EventSource owns
      // reconnection, carrying Last-Event-Id with it).
      timeoutMs: 0,
      retry: false,
      correlationId,
      auth: { cookieHeader: request.headers.get('cookie') ?? '' },
      headers: lastEventId ? { 'Last-Event-Id': lastEventId } : {},
      signal: request.signal,
    })
  } catch {
    return NextResponse.json(
      { error: 'run_events_unavailable' },
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
      // Disable proxy buffering so events flush immediately (nginx/ingress).
      'X-Accel-Buffering': 'no',
      'X-Correlation-Id': correlationId,
    },
  })
}
