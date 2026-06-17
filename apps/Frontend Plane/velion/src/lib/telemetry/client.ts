// G25 — minimal client-side telemetry.
//
// Captures structured browser events for the zero-input enterprise sign-in
// flow (success/failure rates, latencies) and POSTs them to
// `/api/telemetry/events`, which logs structured JSON for downstream
// ingestion (Loki, Datadog, etc.).
//
// Why not OpenTelemetry browser SDK? Three reasons:
//   1. OTEL browser is ~80 KB gzipped; we currently need 4–5 event types,
//      not a full distributed-tracing surface.
//   2. The G15 correlation ID is already propagated server-side; emitting
//      it as an event attribute is enough for stitching frontend + backend.
//   3. Backend aggregation tooling (Loki / metrics) hasn't been chosen.
//      Logging structured JSON server-side keeps options open.
//
// If a future wave needs full OTEL, swap the `emit` implementation and the
// call sites stay unchanged.
//
// Event taxonomy (extend cautiously — name -> meaning is part of the
// observability contract):
//
//   auth.login.completed
//     Fired right after `/auth/callback` finishes. `durationMs` measures
//     from `auth.login.started` to dashboard render-ready. Use for the
//     p50/p95 sign-in→dashboard latency target.
//
//   onboarding.zero_input.resolved
//     Fired when the auto-org/auto-role resolution succeeds (the user lands
//     on /dashboard without seeing the onboarding wizard). `props.orgId` +
//     `props.role` describe the verdict.
//
//   onboarding.zero_input.failed
//     Fired when the resolver returns `needsOnboarding=true` post-OAuth or
//     surfaces a non-OK callback error. `props.reason` carries the cause.
//
//   dashboard.first_paint
//     Fired by the dashboard layout when it has rendered with a live
//     session-context payload. Used as the "first value visible" anchor.

export type TelemetryEventName =
  | 'auth.login.started'
  | 'auth.login.completed'
  | 'onboarding.zero_input.resolved'
  | 'onboarding.zero_input.failed'
  | 'dashboard.first_paint'
  | 'web_vital.recorded'
  | 'route.viewed'
  | 'route.left'
  | 'performance.long_task'
  | 'performance.event_timing'
  | 'sse.started'
  | 'sse.first_byte'
  | 'sse.first_token'
  | 'sse.completed'
  | 'sse.aborted'
  | 'sse.retried'
  | 'mutation.failed'
  | 'embed.opened'
  | 'embed.interactive'
  | 'support.outcome'
  | 'support.csat'

export interface TelemetryEvent {
  name: TelemetryEventName
  /** UTC milliseconds since epoch when the event happened in the browser. */
  ts: number
  /** Optional duration in ms (e.g. login latency). */
  durationMs?: number
  /** G15 correlation id propagated from the inbound request, if known. */
  correlationId?: string
  /** Arbitrary structured props — keep keys snake_case for backend consumers. */
  props?: Record<string, unknown>
}

const BUFFER_KEY = 'velion.telemetry.buffer.v1'
const ENDPOINT = '/api/telemetry/events'

function readCorrelationIdHint(): string | undefined {
  if (typeof document === 'undefined') return undefined
  const match = document.cookie.match(/(?:^|;\s*)x-correlation-id=([^;]+)/i)
  return match ? decodeURIComponent(match[1]) : undefined
}

async function postEvent(event: TelemetryEvent): Promise<boolean> {
  if (typeof window === 'undefined') return false
  try {
    const payload = JSON.stringify(event)
    // Prefer sendBeacon for tab-close resilience; fall back to fetch.
    if (navigator.sendBeacon) {
      const blob = new Blob([payload], { type: 'application/json' })
      if (navigator.sendBeacon(ENDPOINT, blob)) return true
    }
    const response = await fetch(ENDPOINT, {
      method: 'POST',
      credentials: 'include',
      headers: { 'Content-Type': 'application/json' },
      body: payload,
      keepalive: true,
    })
    return response.ok
  } catch {
    return false
  }
}

function readBuffer(): TelemetryEvent[] {
  if (typeof window === 'undefined') return []
  try {
    const raw = window.sessionStorage.getItem(BUFFER_KEY)
    if (!raw) return []
    const parsed = JSON.parse(raw) as unknown
    return Array.isArray(parsed) ? (parsed as TelemetryEvent[]) : []
  } catch {
    return []
  }
}

function writeBuffer(events: TelemetryEvent[]): void {
  if (typeof window === 'undefined') return
  try {
    if (events.length === 0) {
      window.sessionStorage.removeItem(BUFFER_KEY)
    } else {
      // Cap the buffer so a long offline period can't unbounded-grow storage.
      const capped = events.slice(-100)
      window.sessionStorage.setItem(BUFFER_KEY, JSON.stringify(capped))
    }
  } catch {
    // sessionStorage may be unavailable (Safari private). Best-effort only.
  }
}

/**
 * Emit a telemetry event. Best-effort: never throws, never blocks UI.
 *
 * Failures (network down, beacon refused, etc.) are buffered in
 * sessionStorage and retried on the next successful `emit`. Buffer caps at
 * 100 events to avoid runaway memory.
 */
export function emit(
  name: TelemetryEventName,
  attrs?: { durationMs?: number; props?: Record<string, unknown>; correlationId?: string },
): void {
  if (typeof window === 'undefined') return

  const event: TelemetryEvent = {
    name,
    ts: Date.now(),
    durationMs: attrs?.durationMs,
    correlationId: attrs?.correlationId ?? readCorrelationIdHint(),
    props: attrs?.props,
  }

  void (async () => {
    const buffered = readBuffer()
    const queue: TelemetryEvent[] = [...buffered, event]

    const survivors: TelemetryEvent[] = []
    for (const item of queue) {
      const ok = await postEvent(item)
      if (!ok) survivors.push(item)
    }
    writeBuffer(survivors)
  })()
}

/**
 * Convenience wrapper that times an async operation and emits a
 * `<name>.completed` event with `durationMs` measured from invocation.
 *
 * Falls through any error to the caller — telemetry never swallows
 * application errors. On error, emits a `<base>.failed` event with the
 * thrown message in `props.reason`.
 */
export async function withDurationEmit<T>(
  baseName: 'auth.login' | 'onboarding.zero_input',
  fn: () => Promise<T>,
  props?: Record<string, unknown>,
): Promise<T> {
  const start = Date.now()
  emit(`${baseName}.started` as TelemetryEventName, { props })
  try {
    const result = await fn()
    emit(`${baseName}.completed` as TelemetryEventName, {
      durationMs: Date.now() - start,
      props,
    })
    return result
  } catch (error) {
    const reason = error instanceof Error ? error.message : 'unknown'
    emit(`${baseName}.failed` as TelemetryEventName, {
      durationMs: Date.now() - start,
      props: { ...props, reason },
    })
    throw error
  }
}
