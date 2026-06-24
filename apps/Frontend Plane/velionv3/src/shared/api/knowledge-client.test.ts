import { afterEach, describe, expect, it, vi } from 'vitest'
import { streamCrawlRunEvents, type CrawlWorkflowEvent } from './knowledge-client'

function sseResponse(frames: string[]): Response {
  const encoder = new TextEncoder()
  const stream = new ReadableStream<Uint8Array>({
    start(controller) {
      for (const frame of frames) controller.enqueue(encoder.encode(frame))
      controller.close()
    },
  })
  return new Response(stream, {
    status: 200,
    headers: { 'Content-Type': 'text/event-stream' },
  })
}

describe('streamCrawlRunEvents — crawl 0-pages fix', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('surfaces pages_visited from a run_completed event and fires onDone', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      sseResponse([
        // Quarry control Event shape: stage fields live under `payload`,
        // the SSE event name is the event `type`.
        `event: run_completed\ndata: ${JSON.stringify({
          event_id: 'evt_1',
          type: 'run_completed',
          run_id: 'run_1',
          payload: { pages_visited: 5, pages_failed: 1 },
        })}\n\n`,
      ]),
    )
    vi.stubGlobal('fetch', fetchMock)

    const events: CrawlWorkflowEvent[] = []
    const onDone = vi.fn()
    await streamCrawlRunEvents('org_1', 'job_1', {
      onEvent: (event) => events.push(event),
      onDone,
    })

    const completed = events.find((event) => event.status === 'completed')
    expect(completed).toBeDefined()
    expect(completed?.pagesVisited).toBe(5)
    expect(completed?.pagesFailed).toBe(1)
    expect(completed?.detail).toBe('Crawled 5 pages (1 failed)')
    expect(onDone).toHaveBeenCalled()
  })

  it('honors the server-provided event-stream path (job-id keyed)', async () => {
    const fetchMock = vi.fn().mockResolvedValue(sseResponse(['event: done\ndata: done\n\n']))
    vi.stubGlobal('fetch', fetchMock)

    await streamCrawlRunEvents(
      'org_1',
      'job_1',
      { onEvent: () => {} },
      undefined,
      '/api/v1/knowledge/jobs/job_xyz/events',
    )

    const calledUrl = String(fetchMock.mock.calls[0]?.[0] ?? '')
    expect(calledUrl).toContain('/api/v1/knowledge/jobs/job_xyz/events')
  })

  it('falls back to the job-events path (not the broken /runs path)', async () => {
    const fetchMock = vi.fn().mockResolvedValue(sseResponse(['event: done\ndata: done\n\n']))
    vi.stubGlobal('fetch', fetchMock)

    await streamCrawlRunEvents('org_1', 'job_abc', { onEvent: () => {} })

    const calledUrl = String(fetchMock.mock.calls[0]?.[0] ?? '')
    expect(calledUrl).toContain('/api/v1/knowledge/jobs/job_abc/events')
    expect(calledUrl).not.toContain('/runs/')
  })
})
