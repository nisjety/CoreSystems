// @vitest-environment jsdom

import { createRouter, memoryHistory } from '@solidjs/router'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { CoreSidebar } from '@/features/core/components/CoreSidebar'
import { routeFromPath } from '@/features/core/lib/shell-data'
import VerevonIngestionsPage from '@/features/ingestions/components/VerevonIngestionsPage'

function renderIngestions() {
  const TestRouter = createRouter({
    routes: [{ path: '/ingestions', component: VerevonIngestionsPage }],
    history: memoryHistory('/ingestions'),
    explicitLinks: true,
  })
  return render(() => <TestRouter>{(props) => <>{props.children}</>}</TestRouter>)
}

function mockIngestionApi() {
  vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input)

    if (url.endsWith('/api/ingestions/runs')) {
      return jsonResponse([])
    }
    if (url.endsWith('/api/ingestions/schedules')) {
      return jsonResponse([])
    }
    if (url.endsWith('/api/ingestions/sources')) {
      return jsonResponse({ integrations: [], quarrySources: [] })
    }
    if (url.endsWith('/api/ingestions/profiles')) {
      return jsonResponse({ profiles: [] })
    }

    return jsonResponse({ error: { code: 'not_found', message: `Unhandled ${url}` } }, 404)
  }))
}

function jsonResponse(body: unknown, status = 200) {
  return new Response(JSON.stringify({ data: body }), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
})

const BASE_ROUTES: Record<string, unknown> = {
  '/api/ingestions/runs': [],
  '/api/ingestions/schedules': [],
  '/api/ingestions/sources': { integrations: [], quarrySources: [] },
  '/api/ingestions/profiles': { profiles: [] },
  '/api/v1/monitoring/schedules': [],
  '/api/v1/me/session-context': { userId: 'user_1', orgId: 'org_a', orgs: [{ id: 'org_a', name: 'Acme', role: 'owner' }] },
}

/** Answers GETs from BASE_ROUTES (plus overrides); everything else goes to `handle` or 404s. */
function stubRoutes(
  handle: (url: string, method: string, init?: RequestInit) => Response | null = () => null,
  overrides: Record<string, unknown> = {},
) {
  const routes = { ...BASE_ROUTES, ...overrides }
  const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input)
    const method = init?.method ?? 'GET'
    const handled = handle(url, method, init)
    if (handled) return handled
    if (method === 'GET') {
      const match = Object.keys(routes).find((path) => url.endsWith(path))
      if (match) return jsonResponse(routes[match])
    }
    return jsonResponse({ error: { code: 'not_found', message: `Unhandled ${url}` } }, 404)
  })
  vi.stubGlobal('fetch', fetchMock)
  return fetchMock
}

describe('VerevonIngestionsPage', () => {
  it('opens on the playground with the endpoint groups, overview and recent runs', async () => {
    mockIngestionApi()

    renderIngestions()

    expect(screen.getByRole('heading', { name: 'Innhenting', level: 1 })).toBeTruthy()
    expect(screen.getByText(/kjør crawler og uttrekk, inspiser bevis/i)).toBeTruthy()
    expect(screen.getByRole('button', { name: /lekeplass/i }).getAttribute('aria-pressed')).toBe('true')
    expect(screen.getByRole('button', { name: /kjøringer/i })).toBeTruthy()
    expect(screen.getByRole('button', { name: /tidsplaner/i })).toBeTruthy()
    expect(screen.getByRole('button', { name: /overvåking/i })).toBeTruthy()
    expect(screen.getByRole('button', { name: /kilder/i })).toBeTruthy()
    expect(screen.getByRole('button', { name: /profiler/i })).toBeTruthy()
    expect(screen.getByRole('button', { name: /oppdater/i })).toBeTruthy()
    // Firecrawl-style endpoint picker: scrape is the default endpoint.
    expect(screen.getByRole('button', { name: /^scrape$/i }).getAttribute('aria-pressed')).toBe('true')
    for (const name of [/^søk$/i, /^kart$/i, /^uttrekk$/i, /^crawl$/i, /^batch$/i]) {
      expect(screen.getByRole('button', { name })).toBeTruthy()
    }
    expect(screen.getByRole('textbox', { name: /mål-url/i })).toBeTruthy()
    expect(screen.getByRole('button', { name: /start kjøring/i })).toBeTruthy()
    expect(screen.getByRole('button', { name: /hent kode/i })).toBeTruthy()
    expect(screen.getByRole('heading', { name: /nylige kjøringer/i })).toBeTruthy()
    expect(screen.getByRole('link', { name: /åpne kunnskap/i }).getAttribute('href')).toBe('/knowledge')

    await waitFor(() => expect(screen.getByText(/ingen varige kjøringer ennå/i)).toBeTruthy())
  })

  it('scrapes through the knowledge route with the chosen formats and previews the markdown', async () => {
    let scrapeInit: RequestInit | undefined
    const fetchMock = stubRoutes((url, method, init) => {
      if (url.endsWith('/api/v1/knowledge/scrape') && method === 'POST') {
        scrapeInit = init
        return jsonResponse({
          url: { requested: 'https://example.com/', final_url: 'https://example.com/' },
          status: 200,
          markdown: '# Example Domain\n\nThis domain is for use in examples.',
          metadata: { title: 'Example Domain', lang: 'en' },
          driver: { kind: 'static', duration_ms: 153 },
          formats: { links: [{ href: 'https://iana.org/domains', text: 'More' }] },
          fingerprint: 'blake3:f26a1fdd0ce5f1e0fb08',
        })
      }
      return null
    })

    renderIngestions()
    await waitFor(() => expect(screen.getByText(/ingen varige kjøringer ennå/i)).toBeTruthy())
    // The org comes from the session context; wait until it has been read.
    await waitFor(() => expect(fetchMock.mock.calls.some(([input]) => String(input).endsWith('/api/v1/me/session-context'))).toBe(true))
    await new Promise((resolve) => setTimeout(resolve, 0))

    fireEvent.click(screen.getByRole('button', { name: /^format/i }))
    flush()
    fireEvent.click(screen.getByRole('button', { name: /^lenker$/i }))
    flush()
    fireEvent.input(screen.getByRole('textbox', { name: /mål-url/i }), { target: { value: 'https://example.com' } })
    flush()
    fireEvent.click(screen.getByRole('button', { name: /start kjøring/i }))

    await waitFor(() => expect(screen.getByText(/this domain is for use in examples/i)).toBeTruthy())
    expect(new Headers(scrapeInit?.headers).get('x-verevon-org-id')).toBe('org_a')
    const body = JSON.parse(String(scrapeInit?.body)) as Record<string, unknown>
    expect(body).toMatchObject({ url: 'https://example.com', formats: ['markdown', 'links'], only_main_content: true })
    expect(body).not.toHaveProperty('ingest')
    expect(screen.getByRole('button', { name: /lenker \(1\)/i })).toBeTruthy()
    expect(screen.getByRole('button', { name: /send til kunnskap/i })).toBeTruthy()
  })

  it('reconciles a false-failure 502 by checking whether the batch was actually started', async () => {
    let created = false
    const newRun = {
      id: 'job_new_1',
      kind: 'batch',
      status: 'queued',
      createdAt: new Date().toISOString(),
      target: '2 URLs',
      progress: {},
      stats: {},
    }
    stubRoutes((url, method) => {
      if (url.endsWith('/api/ingestions/runs') && method === 'POST') {
        // The batch reaches the backend and is durably created, but the
        // response itself is lost to a transient gateway error.
        created = true
        return jsonResponse({ message: 'Bad Gateway' }, 502)
      }
      if (url.endsWith('/api/ingestions/runs')) return jsonResponse(created ? [newRun] : [])
      return null
    })

    renderIngestions()
    await waitFor(() => expect(screen.getByText(/ingen varige kjøringer ennå/i)).toBeTruthy())

    fireEvent.click(screen.getByRole('button', { name: /^batch$/i }))
    flush()
    fireEvent.input(screen.getByRole('textbox', { name: /url-er/i }), {
      target: { value: 'https://example.com/a\nhttps://example.com/b' },
    })
    flush()
    fireEvent.click(screen.getByRole('button', { name: /start batch/i }))

    // The reconciled run renders in the job receipt and the recent-runs strip.
    await waitFor(() => expect(screen.getByRole('heading', { name: /batch startet/i })).toBeTruthy())
    expect(screen.getAllByText('2 URLs').length).toBeGreaterThan(0)
    expect(screen.queryByRole('alert')).toBeNull()
  })

  it('shows a real failure when starting a batch genuinely did not go through', async () => {
    stubRoutes((url, method) => {
      if (url.endsWith('/api/ingestions/runs') && method === 'POST') {
        return jsonResponse({ message: 'Bad Gateway' }, 502)
      }
      return null
    })

    renderIngestions()
    await waitFor(() => expect(screen.getByText(/ingen varige kjøringer ennå/i)).toBeTruthy())

    fireEvent.click(screen.getByRole('button', { name: /^batch$/i }))
    flush()
    fireEvent.input(screen.getByRole('textbox', { name: /url-er/i }), { target: { value: 'https://example.com/a' } })
    flush()
    fireEvent.click(screen.getByRole('button', { name: /start batch/i }))

    expect(await screen.findByRole('alert')).toBeTruthy()
    expect(screen.queryByRole('heading', { name: /batch startet/i })).toBeNull()
  })

  it('filters the run history from an overview tile and opens a run with its log', async () => {
    const runs = [
      { id: 'job_ok', runId: 'run_ok', kind: 'crawl', status: 'completed', createdAt: '2026-10-28T10:00:00Z', startedAt: '2026-10-28T10:00:00Z', completedAt: '2026-10-28T10:01:05Z', target: 'https://docs.example.com', progress: { completed: 8, total: 8 }, stats: { completed: 8 } },
      { id: 'job_bad', runId: 'run_bad', kind: 'batch', status: 'failed', createdAt: '2026-10-28T09:00:00Z', target: '3 URLs', progress: {}, stats: { failed: 3 } },
    ]
    const evidence = {
      runId: 'run_bad',
      timeline: [{ run_id: 'run_bad', kind: 'batch', stage: 'fetch', status: 'failed', seq: 1, completed: 0, total: 3, discovered: 3, queued: 0, retries: 2, blocks: 1, timestamp: '2026-10-28T09:00:05Z' }],
      warnings: [{ id: 'w1', stage: 'fetch', summary: 'Origin refused the request' }],
    }
    stubRoutes((url) => (url.includes('/api/ingestions/evidence?runId=run_bad') ? jsonResponse(evidence) : null), {
      '/api/ingestions/runs': runs,
    })

    renderIngestions()
    await waitFor(() => expect(screen.getAllByText('https://docs.example.com').length).toBeGreaterThan(0))

    fireEvent.click(screen.getByRole('button', { name: /vis feilede/i }))
    flush()

    expect(screen.getByRole('button', { name: /kjøringer/i }).getAttribute('aria-pressed')).toBe('true')
    expect(screen.getByRole('heading', { name: /kjøringshistorikk/i })).toBeTruthy()
    expect(screen.queryByText('https://docs.example.com')).toBeNull()

    fireEvent.click(screen.getByText('3 URLs'))
    flush()
    fireEvent.click(screen.getByRole('button', { name: /^logg/i }))
    flush()

    await waitFor(() => expect(screen.getByText('Origin refused the request')).toBeTruthy())
    expect(screen.getByText(/2 nye forsøk/)).toBeTruthy()
  })

  it('creates a recurring monitor and renders the change timeline of an on-demand check', async () => {
    const checkBody = {
      sourceUrl: 'https://example.com/',
      orgId: 'org_a',
      status: 'changed',
      newBaseline: null,
      prevBaseline: null,
      diffId: null,
      checkedAt: '2026-06-19T12:00:00Z',
    }
    const historyBody = [
      { baselineId: 'bln_2', orgId: 'org_a', sourceUrl: 'https://example.com/', fingerprint: 'blake3:feedfacefeed00', capturedAt: '2026-06-19T12:00:00Z' },
      { baselineId: 'bln_1', orgId: 'org_a', sourceUrl: 'https://example.com/', fingerprint: 'blake3:deadbeefcafef00d', capturedAt: '2026-06-18T12:00:00Z' },
    ]
    let monitors: unknown[] = []
    let createBody: unknown = null

    stubRoutes((url, method, init) => {
      if (url.endsWith('/api/v1/monitoring/schedules') && method === 'POST') {
        createBody = JSON.parse(String(init?.body))
        monitors = [{ id: 'sch_1', target_kind: 'change_monitor', target_ref: 'https://example.com/', cron: '0 7 * * *', enabled: true, created_at: 1_761_000_000 }]
        return jsonResponse(monitors[0], 201)
      }
      if (url.endsWith('/api/v1/monitoring/schedules')) return jsonResponse(monitors)
      if (url.endsWith('/api/v1/monitoring/check') && method === 'POST') return jsonResponse(checkBody)
      if (url.includes('/api/v1/monitoring/history')) return jsonResponse(historyBody)
      return null
    })

    renderIngestions()

    fireEvent.click(screen.getByRole('button', { name: /overvåking/i }))
    flush()

    expect(screen.getByRole('heading', { name: /sjekk en side for endringer/i })).toBeTruthy()
    await waitFor(() => expect(screen.getByText(/ingen overvåkere ennå/i)).toBeTruthy())

    fireEvent.input(screen.getByRole('textbox', { name: /side-url/i }), { target: { value: 'https://example.com/' } })
    flush()
    fireEvent.click(screen.getByRole('button', { name: /opprett overvåker/i }))

    await waitFor(() => expect(screen.getByText(/hver dag kl\. 07:00/i)).toBeTruthy())
    expect(createBody).toEqual({ url: 'https://example.com/', preset: 'daily' })

    fireEvent.click(screen.getByRole('button', { name: /sjekk nå/i }))
    flush()

    await waitFor(() => expect(screen.getByText(/siste sjekk/i)).toBeTruthy())
    await waitFor(() => expect(screen.getByText(/innhold endret/i)).toBeTruthy())
    expect(screen.getByText(/første basislinje/i)).toBeTruthy()
    expect(screen.getByText(/blake3:deadbeefcafe/i)).toBeTruthy()
  })

  it('creates and deletes a tracked web source from the Sources tab', async () => {
    const sourceRecord = {
      id: 'src_01',
      name: 'Acme pricing',
      url: 'https://acme.example/pricing',
      kind: 'scrape',
      status: 'active',
      createdAt: '',
      updatedAt: '',
      config: {},
    }
    let createdOnce = false
    const calls: Array<{ url: string; method: string; body: string }> = []

    vi.stubGlobal(
      'fetch',
      vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
        const url = String(input)
        const method = init?.method ?? 'GET'
        calls.push({ url, method, body: typeof init?.body === 'string' ? init.body : '' })

        if (url.endsWith('/api/ingestions/runs')) return jsonResponse([])
        if (url.endsWith('/api/ingestions/schedules')) return jsonResponse([])
        if (url.endsWith('/api/ingestions/profiles')) return jsonResponse({ profiles: [] })
        // POST create → mark created so the next list reflects it (no fabrication).
        if (url.endsWith('/api/ingestions/sources') && method === 'POST') {
          createdOnce = true
          return jsonResponse({ source: sourceRecord }, 201)
        }
        // DELETE → 204-style success; subsequent list is empty again.
        if (url.includes('/api/ingestions/sources/') && method === 'DELETE') {
          createdOnce = false
          return jsonResponse({ deleted: true, sourceId: 'src_01' })
        }
        if (url.endsWith('/api/ingestions/sources')) {
          return jsonResponse({
            integrations: [],
            quarrySources: createdOnce ? [sourceRecord] : [],
          })
        }
        return jsonResponse({ error: { code: 'not_found', message: `Unhandled ${url}` } }, 404)
      }),
    )

    renderIngestions()
    fireEvent.click(screen.getByRole('button', { name: /kilder/i }))

    // Honest empty state before anything is registered.
    await waitFor(() => expect(screen.getByText(/ingen varige kilderegistreringer ennå/i)).toBeTruthy())

    // Fill + submit the create form.
    fireEvent.input(screen.getByRole('textbox', { name: /^navn$/i }), {
      target: { value: 'Acme pricing' },
    })
    flush()
    fireEvent.input(screen.getByRole('textbox', { name: /^url$/i }), {
      target: { value: 'https://acme.example/pricing' },
    })
    flush()
    fireEvent.click(screen.getByRole('button', { name: /legg til kilde/i }))
    flush()

    // The newly created source appears after the post-create refetch.
    await waitFor(() => expect(screen.getByText('Acme pricing')).toBeTruthy())

    const createCall = calls.find((c) => c.url.endsWith('/api/ingestions/sources') && c.method === 'POST')
    expect(createCall).toBeTruthy()
    const sentBody = JSON.parse(createCall!.body)
    expect(sentBody.name).toBe('Acme pricing')
    expect(sentBody.url).toBe('https://acme.example/pricing')
    expect(sentBody.kind).toBe('crawl')
    // IDOR hygiene at the client boundary: no org field is ever sent.
    expect('org_id' in sentBody).toBe(false)
    expect('orgId' in sentBody).toBe(false)

    // Remove it; the list returns to the honest empty state.
    fireEvent.click(screen.getByRole('button', { name: /fjern/i }))
    await waitFor(() => expect(screen.getByText(/ingen varige kilderegistreringer ennå/i)).toBeTruthy())

    const deleteCall = calls.find((c) => c.url.includes('/api/ingestions/sources/') && c.method === 'DELETE')
    expect(deleteCall).toBeTruthy()
    expect(deleteCall!.url).toContain('/api/ingestions/sources/src_01')
  })

  it('reconciles a false-failure 502 by checking whether the source was actually removed', async () => {
    const sourceRecord = {
      id: 'src_01', name: 'Acme pricing', url: 'https://acme.example/pricing',
      kind: 'scrape', status: 'active', createdAt: '', updatedAt: '', config: {},
    }
    let removed = false
    vi.stubGlobal(
      'fetch',
      vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
        const url = String(input)
        const method = init?.method ?? 'GET'
        if (url.endsWith('/api/ingestions/runs')) return jsonResponse([])
        if (url.endsWith('/api/ingestions/schedules')) return jsonResponse([])
        if (url.endsWith('/api/ingestions/profiles')) return jsonResponse({ profiles: [] })
        if (url.includes('/api/ingestions/sources/') && method === 'DELETE') {
          // The removal reaches the backend and is durably recorded, but the
          // response itself is lost to a transient gateway error.
          removed = true
          return jsonResponse({ message: 'Bad Gateway' }, 502)
        }
        if (url.endsWith('/api/ingestions/sources')) {
          return jsonResponse({ integrations: [], quarrySources: removed ? [] : [sourceRecord] })
        }
        return jsonResponse({ error: { code: 'not_found', message: `Unhandled ${url}` } }, 404)
      }),
    )

    renderIngestions()
    fireEvent.click(screen.getByRole('button', { name: /kilder/i }))
    await waitFor(() => expect(screen.getByText('Acme pricing')).toBeTruthy())

    fireEvent.click(screen.getByRole('button', { name: /fjern/i }))

    await waitFor(() => expect(screen.getByText(/ingen varige kilderegistreringer ennå/i)).toBeTruthy())
    expect(screen.queryByRole('alert')).toBeNull()
  })

  it('shows a real failure when removing a source genuinely did not go through', async () => {
    const sourceRecord = {
      id: 'src_01', name: 'Acme pricing', url: 'https://acme.example/pricing',
      kind: 'scrape', status: 'active', createdAt: '', updatedAt: '', config: {},
    }
    vi.stubGlobal(
      'fetch',
      vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
        const url = String(input)
        const method = init?.method ?? 'GET'
        if (url.endsWith('/api/ingestions/runs')) return jsonResponse([])
        if (url.endsWith('/api/ingestions/schedules')) return jsonResponse([])
        if (url.endsWith('/api/ingestions/profiles')) return jsonResponse({ profiles: [] })
        if (url.includes('/api/ingestions/sources/') && method === 'DELETE') {
          return jsonResponse({ message: 'Bad Gateway' }, 502)
        }
        if (url.endsWith('/api/ingestions/sources')) {
          // Still present — the removal genuinely didn't land.
          return jsonResponse({ integrations: [], quarrySources: [sourceRecord] })
        }
        return jsonResponse({ error: { code: 'not_found', message: `Unhandled ${url}` } }, 404)
      }),
    )

    renderIngestions()
    fireEvent.click(screen.getByRole('button', { name: /kilder/i }))
    await waitFor(() => expect(screen.getByText('Acme pricing')).toBeTruthy())

    fireEvent.click(screen.getByRole('button', { name: /fjern/i }))

    expect(await screen.findByRole('alert')).toBeTruthy()
    expect(screen.getByText('Acme pricing')).toBeTruthy()
  })

  it('keeps the v2 ingestion sidebar menu available on the v3 shell', () => {
    const TestRouter = createRouter({
      explicitLinks: true,
      routes: [{
        path: '/*all',
        component: () => (
          <CoreSidebar
            activeRoute={routeFromPath('/ingestions')}
            expanded
            onExpandedChange={vi.fn()}
            onOpenSearch={vi.fn()}
          />
        ),
      }],
      history: memoryHistory('/ingestions'),
    })
    render(() => <TestRouter>{(props) => <>{props.children}</>}</TestRouter>)

    const navigation = screen.getByRole('navigation', { name: /innhenting navigasjon/i })
    expect(within(navigation).getByRole('link', { name: /^arbeidsflate$/i }).getAttribute('href')).toBe('/ingestions')
    expect(within(navigation).getByRole('link', { name: /^kunnskap$/i }).getAttribute('href')).toBe('/knowledge')
    // "Spør Verevon" was one of five `/chat` shortcuts scattered through
    // unrelated sidebar sections. Chat has exactly one destination now (its own
    // section); a surface that wants Verevon's help launches a chat of its own
    // rather than linking sideways. See chat-route-ownership.test.ts.
    expect(within(navigation).queryByRole('link', { name: /^spør verevon$/i })).toBeNull()
  })
})
