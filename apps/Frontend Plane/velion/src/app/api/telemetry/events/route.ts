import { NextRequest, NextResponse } from 'next/server'

import { getCorrelationId } from '../../_lib/control-plane-auth'

// G25 — server-side telemetry sink.
//
// Accepts structured browser events from `src/lib/telemetry/client.ts` and
// logs them as single-line JSON to stdout. The container log driver
// (json-file by default in dev) picks these up; Loki / Datadog ingestion is
// a downstream concern.
//
// Why a thin proxy and not a direct fetch from the browser to an external
// telemetry endpoint? Three reasons:
//   - keeps PII inside the verevon-net (no third-party host learns user
//     attributes from the browser),
//   - lets us enrich every event with the G15 correlation id server-side,
//   - gives us one place to add sampling / rate limiting / DLP if the event
//     volume grows.
//
// This route is intentionally **not** session-gated. Anonymous events (e.g.
// `auth.login.started` before sign-in finishes) must reach the sink. The
// risk is bounded by:
//   - bodies are size-capped (8 KB),
//   - we log the event as data, not as control flow,
//   - downstream aggregation can drop unauthenticated noise if needed.

interface IncomingEvent {
  name?: string
  ts?: number
  durationMs?: number
  correlationId?: string
  props?: Record<string, unknown>
}

const MAX_BODY_BYTES = 8 * 1024

const KNOWN_EVENT_NAMES = new Set([
  'auth.login.started',
  'auth.login.completed',
  'onboarding.zero_input.resolved',
  'onboarding.zero_input.failed',
  'dashboard.first_paint',
  'web_vital.recorded',
  'route.viewed',
  'route.left',
  'performance.long_task',
  'performance.event_timing',
  'sse.started',
  'sse.first_byte',
  'sse.first_token',
  'sse.completed',
  'sse.aborted',
  'sse.retried',
  'mutation.failed',
  'embed.opened',
  'embed.interactive',
  'support.outcome',
  'support.csat',
])

export async function POST(request: NextRequest): Promise<NextResponse> {
  // Size guard — small JSON only.
  const contentLength = Number(request.headers.get('content-length') ?? 0)
  if (contentLength > MAX_BODY_BYTES) {
    return NextResponse.json({ error: 'payload too large' }, { status: 413 })
  }

  let event: IncomingEvent
  try {
    event = (await request.json()) as IncomingEvent
  } catch {
    return NextResponse.json({ error: 'invalid json' }, { status: 400 })
  }

  if (!event.name || typeof event.name !== 'string') {
    return NextResponse.json({ error: 'event.name required' }, { status: 400 })
  }
  if (!KNOWN_EVENT_NAMES.has(event.name)) {
    // Unknown events are accepted but tagged — keeps the taxonomy honest
    // without breaking forward compatibility when the client ships a new
    // event ahead of the route.
    event.props = { ...(event.props ?? {}), _unknown_event: true }
  }

  const correlationId = event.correlationId ?? getCorrelationId(request)

  // Single-line JSON for structured-log ingestion.
  const record = {
    level: 'info',
    msg: 'telemetry.event',
    event: event.name,
    ts: event.ts ?? Date.now(),
    duration_ms: event.durationMs,
    correlation_id: correlationId,
    user_agent: request.headers.get('user-agent') ?? undefined,
    props: event.props,
  }
  // eslint-disable-next-line no-console -- structured-log sink, intentional
  console.log(JSON.stringify(record))

  return NextResponse.json({ ok: true })
}
