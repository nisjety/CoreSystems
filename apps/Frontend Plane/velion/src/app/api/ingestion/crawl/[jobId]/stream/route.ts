import { NextRequest } from 'next/server'

// G32: Quarry-v2 has no native SSE endpoint. Events live at the paginated JSON
// route `GET /v1/jobs/{id}/events?after_seq={n}&limit={k}`. This route polls
// that endpoint and synthesizes the SSE event shape the browser
// (CrawlProgressContext.tsx) already expects.
const QUARRY_URL = process.env.QUARRY_API_URL || 'http://quarry-control:8081'

export const runtime = 'nodejs'
export const dynamic = 'force-dynamic'

// Quarry-v2 job IDs are ULID-prefixed, e.g. `job_01KRA09W9N17J4BJ8AAMG6XZDJ`.
const JOB_ID_PATTERN = /^job_[0-9A-HJ-NP-TV-Z]{26}$/

const POLL_INTERVAL_MS = 750
const PAGE_LIMIT = 50

type QuarryEvent = {
  event_id: string
  job_id?: string
  run_id?: string
  type: string
  ts?: string
  seq: number
  payload?: Record<string, unknown>
}

type QuarryJobStatus = {
  id: string
  kind: string
  status: string
  created_at?: number
}

type QuarryEnvelope<T> = {
  data?: T | null
  meta?: Record<string, unknown> | null
  error?: { code?: string; message?: string } | null
}

const TERMINAL_RUN_EVENTS = new Set(['run_completed', 'run_failed', 'run_cancelled'])
const TERMINAL_STATUSES = new Set(['completed', 'failed', 'cancelled'])

function sse(event: string, data: unknown): string {
  return `event: ${event}\ndata: ${JSON.stringify(data)}\n\n`
}

async function fetchEventsPage(
  jobId: string,
  afterSeq: number,
  signal: AbortSignal,
): Promise<{ events: QuarryEvent[]; ok: boolean }> {
  const url = `${QUARRY_URL}/v1/jobs/${jobId}/events?after_seq=${afterSeq}&limit=${PAGE_LIMIT}`
  const response = await fetch(url, {
    headers: { Accept: 'application/json' },
    signal,
  })
  if (!response.ok) return { events: [], ok: false }
  const envelope = (await response.json().catch(() => null)) as
    | QuarryEnvelope<QuarryEvent[]>
    | null
  return { events: envelope?.data ?? [], ok: true }
}

async function fetchJobStatus(
  jobId: string,
  signal: AbortSignal,
): Promise<QuarryJobStatus | null> {
  const response = await fetch(`${QUARRY_URL}/v1/jobs/${jobId}`, {
    headers: { Accept: 'application/json' },
    signal,
  })
  if (!response.ok) return null
  const envelope = (await response.json().catch(() => null)) as
    | QuarryEnvelope<QuarryJobStatus>
    | null
  return envelope?.data ?? null
}

export async function GET(
  request: NextRequest,
  { params }: { params: Promise<{ jobId: string }> },
) {
  const { jobId } = await params

  if (!jobId || typeof jobId !== 'string' || !JOB_ID_PATTERN.test(jobId)) {
    return new Response('Invalid jobId', { status: 400 })
  }

  const encoder = new TextEncoder()

  const stream = new ReadableStream({
    async start(controller) {
      let afterSeq = 0
      let pageCount = 0
      let closed = false

      const abortController = new AbortController()
      const onAbort = () => abortController.abort()
      request.signal.addEventListener('abort', onAbort)

      const safeEnqueue = (chunk: string) => {
        if (closed) return
        try {
          controller.enqueue(encoder.encode(chunk))
        } catch {
          closed = true
        }
      }

      const finish = (status: 'completed' | 'failed' | 'cancelled', payload: unknown = {}) => {
        safeEnqueue(sse(status, { ...(payload as object), pageCount }))
        safeEnqueue(sse('heartbeat', { status }))
        closed = true
      }

      try {
        // Emit an initial heartbeat so the client knows the stream is live.
        safeEnqueue(sse('heartbeat', { status: 'connected' }))

        while (!closed) {
          const { events, ok } = await fetchEventsPage(jobId, afterSeq, abortController.signal)
          if (!ok) {
            // Upstream failed — surface and stop.
            safeEnqueue(sse('error', { message: 'upstream events fetch failed' }))
            finish('failed')
            break
          }

          for (const evt of events) {
            if (typeof evt.seq === 'number' && evt.seq > afterSeq) afterSeq = evt.seq

            switch (evt.type) {
              case 'page_fetched': {
                pageCount += 1
                safeEnqueue(sse('page_completed', { ...evt.payload, pageCount }))
                break
              }
              case 'run_started': {
                safeEnqueue(sse('run_started', { ...evt.payload }))
                break
              }
              case 'run_completed': {
                finish('completed', evt.payload)
                break
              }
              case 'run_failed': {
                finish('failed', evt.payload)
                break
              }
              case 'run_cancelled': {
                finish('cancelled', evt.payload)
                break
              }
              default: {
                // Forward unknown event types verbatim — clients can ignore.
                safeEnqueue(sse(evt.type, { ...evt.payload }))
              }
            }

            if (TERMINAL_RUN_EVENTS.has(evt.type)) break
          }

          if (closed) break

          // No events came through this tick — check job status in case the
          // job finished without emitting a terminal event (e.g. very short
          // crawl that was reaped before we polled).
          if (events.length === 0) {
            const job = await fetchJobStatus(jobId, abortController.signal)
            if (job && TERMINAL_STATUSES.has(job.status)) {
              finish(job.status as 'completed' | 'failed' | 'cancelled')
              break
            }
            safeEnqueue(sse('heartbeat', { status: job?.status ?? 'running' }))
          }

          await new Promise((resolve) => setTimeout(resolve, POLL_INTERVAL_MS))
        }
      } catch (err) {
        if ((err as { name?: string }).name !== 'AbortError') {
          safeEnqueue(sse('error', { message: (err as Error).message ?? 'stream error' }))
        }
      } finally {
        request.signal.removeEventListener('abort', onAbort)
        try {
          controller.close()
        } catch {
          // Already closed — ignore.
        }
      }
    },
  })

  return new Response(stream, {
    headers: {
      'Content-Type': 'text/event-stream',
      'Cache-Control': 'no-cache, no-transform',
      Connection: 'keep-alive',
      'X-Accel-Buffering': 'no',
    },
  })
}
