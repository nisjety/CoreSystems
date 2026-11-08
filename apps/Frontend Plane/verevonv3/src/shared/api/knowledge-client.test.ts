import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  buildCrawlScopeOptions,
  CRAWL_MAX_DEPTH,
  CRAWL_PATTERN_MAX_COUNT,
  CRAWL_PATTERN_MAX_LENGTH,
  getDocument,
  getWikiPageByPath,
  listDocuments,
  listWikiPages,
  parseCrawlPatterns,
  startCrawl,
  streamCrawlRunEvents,
  type CrawlWorkflowEvent,
} from './knowledge-client'

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

  it('reads the robots refusals of a run_completed event apart from the other blocked pages', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      sseResponse([
        `event: run_completed\ndata: ${JSON.stringify({
          event_id: 'evt_2',
          type: 'run_completed',
          run_id: 'run_2',
          payload: { pages_visited: 4, pages_blocked: 3, pages_blocked_robots: 2 },
        })}\n\n`,
      ]),
    )
    vi.stubGlobal('fetch', fetchMock)

    const events: CrawlWorkflowEvent[] = []
    await streamCrawlRunEvents('org_1', 'job_2', { onEvent: (event) => events.push(event) })

    const completed = events.find((event) => event.status === 'completed')
    expect(completed?.pagesBlocked).toBe(3)
    expect(completed?.pagesBlockedByRobots).toBe(2)
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

describe('knowledge Data Plane response normalization', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('maps documents-api list and get envelopes into the frontend contract', async () => {
    const record = {
      document_id: 'doc-1',
      title: 'Runbook',
      source: 'sharepoint',
      type: 'pdf',
      created_at: '2026-07-15T00:00:00Z',
      updated_at: '2026-07-15T01:00:00Z',
    }
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(new Response(JSON.stringify({ documents: [record], total: 1 }), { status: 200 }))
      .mockResolvedValueOnce(new Response(JSON.stringify(record), { status: 200 }))
    vi.stubGlobal('fetch', fetchMock)

    const [listed, loaded] = await Promise.all([
      listDocuments('org-1'),
      getDocument('org-1', 'doc-1'),
    ])

    expect(listed[0]).toMatchObject({ id: 'doc-1', sourceId: 'sharepoint', kind: 'pdf' })
    expect(loaded).toMatchObject({ id: 'doc-1', title: 'Runbook' })
  })

  it('maps wiki-store page/version envelopes into the frontend contract', async () => {
    const page = {
      page_id: 'page-1',
      org_id: 'org-1',
      title: 'Operations',
      path: '/operations',
      updated_at: '2026-07-15T01:00:00Z',
    }
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(new Response(JSON.stringify({ pages: [page], total: 1 }), { status: 200 }))
      .mockResolvedValueOnce(new Response(JSON.stringify({ page, version: { content: 'Current guidance' } }), { status: 200 }))
    vi.stubGlobal('fetch', fetchMock)

    const listed = await listWikiPages('org-1')
    const loaded = await getWikiPageByPath('org-1', '/operations')

    expect(listed[0]).toMatchObject({ id: 'page-1', orgId: 'org-1', path: '/operations' })
    expect(loaded.excerpt).toBe('Current guidance')
  })
})

// W2-U65-51: the crawl scope options the composer sends.
describe('crawl scope options (W2-U65-51)', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('splits patterns on lines and top-level commas, keeps brace commas, drops blanks and repeats', () => {
    expect(parseCrawlPatterns('/docs/**, /blog/**\n\n  /docs/**  \r\nre:/tag/.*')).toEqual([
      '/docs/**',
      '/blog/**',
      're:/tag/.*',
    ])
    expect(parseCrawlPatterns('/a/*.{html,htm}, /b/**')).toEqual(['/a/*.{html,htm}', '/b/**'])
    expect(parseCrawlPatterns('   ')).toEqual([])
  })

  it('applies the edge caps: pattern length and pattern count', () => {
    expect(parseCrawlPatterns('x'.repeat(CRAWL_PATTERN_MAX_LENGTH + 1))).toEqual([])
    expect(parseCrawlPatterns('x'.repeat(CRAWL_PATTERN_MAX_LENGTH))).toHaveLength(1)
    const many = Array.from({ length: CRAWL_PATTERN_MAX_COUNT + 20 }, (_, index) => `/p${index}/**`).join('\n')
    expect(parseCrawlPatterns(many)).toHaveLength(CRAWL_PATTERN_MAX_COUNT)
  })

  it('builds only the options the user set', () => {
    expect(buildCrawlScopeOptions({})).toEqual({})
    expect(buildCrawlScopeOptions({ allowSubdomains: false, excludeText: ' ', includeText: '', maxDepth: undefined })).toEqual({})
    expect(buildCrawlScopeOptions({ maxDepth: Number.NaN })).toEqual({})
    expect(buildCrawlScopeOptions({ maxDepth: 0 })).toEqual({})
    expect(buildCrawlScopeOptions({ maxDepth: 99 })).toEqual({ maxDepth: CRAWL_MAX_DEPTH })
    expect(
      buildCrawlScopeOptions({ allowSubdomains: true, excludeText: '/admin/**', includeText: '/docs/**', maxDepth: 2 }),
    ).toEqual({ allowSubdomains: true, excludePaths: ['/admin/**'], includePaths: ['/docs/**'], maxDepth: 2 })
  })

  it('startCrawl sends the scope as typed and nothing else', async () => {
    const fetchMock = vi.fn(
      async (_input: RequestInfo | URL, _init?: RequestInit) =>
        new Response(JSON.stringify({ data: { id: 'run-1', status: 'running' } }), {
          headers: { 'Content-Type': 'application/json' },
          status: 200,
        }),
    )
    vi.stubGlobal('fetch', fetchMock)
    await startCrawl('org-1', {
      url: 'https://example.com',
      ...buildCrawlScopeOptions({ includeText: '/docs/**', maxDepth: 2 }),
    })
    const sent = JSON.parse(String(fetchMock.mock.calls[0]?.[1]?.body))
    expect(sent).toEqual({ includePaths: ['/docs/**'], maxDepth: 2, url: 'https://example.com' })
  })
})
