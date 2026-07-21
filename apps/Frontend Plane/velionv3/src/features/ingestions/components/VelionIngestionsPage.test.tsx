// @vitest-environment jsdom

import { Route, Router } from '@solidjs/router'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { CoreSidebar } from '@/features/core/components/CoreSidebar'
import { routeFromPath } from '@/features/core/lib/shell-data'
import VelionIngestionsPage from '@/features/ingestions/components/VelionIngestionsPage'

function renderIngestions() {
  window.history.pushState(null, '', '/ingestions')

  return render(() => (
    <Router root={(props) => <>{props.children}</>}>
      <Route path="/ingestions" component={VelionIngestionsPage} />
    </Router>
  ))
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

describe('VelionIngestionsPage', () => {
  it('renders the v2 ingestion workspace shell in Solid', async () => {
    mockIngestionApi()
    renderIngestions()

    // Component default locale is Norwegian ('no'); i18n.tr(no, en) renders the
    // Norwegian string by default, so assertions match the rendered Norwegian text.
    expect(screen.getByRole('heading', { name: 'Innhenting', level: 1 })).toBeTruthy()
    expect(screen.getByText(/kjør crawler og uttrekk, inspiser bevis/i)).toBeTruthy()
    expect(screen.getByRole('button', { name: /kjøringer/i }).getAttribute('aria-pressed')).toBe('true')
    expect(screen.getByRole('button', { name: /tidsplaner/i })).toBeTruthy()
    expect(screen.getByRole('button', { name: /kilder/i })).toBeTruthy()
    expect(screen.getByRole('button', { name: /bevis/i })).toBeTruthy()
    expect(screen.getByRole('button', { name: /profiler/i })).toBeTruthy()
    expect(screen.getByRole('button', { name: /oppdater/i })).toBeTruthy()
    expect(screen.getByRole('heading', { name: /start en kjøring/i })).toBeTruthy()
    expect((screen.getByRole('combobox', { name: /kjøringstype/i }) as HTMLSelectElement).value).toBe('scrape')
    expect(screen.getByRole('textbox', { name: /mål-url/i })).toBeTruthy()
    expect(screen.getByRole('button', { name: /start kjøring/i })).toBeTruthy()
    expect(screen.getByRole('heading', { name: /nylige kjøringer/i })).toBeTruthy()
    expect(screen.getByRole('link', { name: /åpne kunnskap/i }).getAttribute('href')).toBe('/knowledge')

    await waitFor(() => expect(screen.getByText(/ingen varige kjøringer ennå/i)).toBeTruthy())
  })

  it('runs an on-demand monitoring check and renders real history (no scheduling control)', async () => {
    const checkBody = {
      sourceUrl: 'https://example.com/',
      orgId: 'org_a',
      status: 'new',
      newBaseline: null,
      prevBaseline: null,
      diffId: null,
      checkedAt: '2026-06-19T12:00:00Z',
    }
    const historyBody = [
      {
        baselineId: 'bln_1',
        orgId: 'org_a',
        sourceUrl: 'https://example.com/',
        fingerprint: 'blake3:deadbeefcafef00d',
        artifactId: null,
        prevBaselineId: null,
        runId: null,
        capturedAt: '2026-06-19T12:00:00Z',
      },
    ]

    vi.stubGlobal(
      'fetch',
      vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
        const url = String(input)
        if (url.endsWith('/api/ingestions/runs')) return jsonResponse([])
        if (url.endsWith('/api/ingestions/schedules')) return jsonResponse([])
        if (url.endsWith('/api/ingestions/sources')) return jsonResponse({ integrations: [], quarrySources: [] })
        if (url.endsWith('/api/ingestions/profiles')) return jsonResponse({ profiles: [] })
        if (url.endsWith('/api/v1/monitoring/check') && init?.method === 'POST') return jsonResponse(checkBody)
        if (url.includes('/api/v1/monitoring/history')) return jsonResponse(historyBody)
        return jsonResponse({ error: { code: 'not_found', message: `Unhandled ${url}` } }, 404)
      }),
    )

    renderIngestions()

    fireEvent.click(screen.getByRole('button', { name: /overvåking/i }))

    expect(screen.getByRole('heading', { name: /sjekk en side for endringer/i })).toBeTruthy()
    // On-demand only: there must be no cron / schedule affordance on this surface.
    expect(screen.queryByText(/cron/i)).toBeNull()
    expect(screen.queryByRole('button', { name: /save schedule/i })).toBeNull()

    const urlField = screen.getByRole('textbox', { name: /side-url/i })
    fireEvent.input(urlField, { target: { value: 'https://example.com/' } })
    fireEvent.click(screen.getByRole('button', { name: /sjekk nå/i }))

    // The live result and the real baseline history both render.
    await waitFor(() => expect(screen.getByText(/siste sjekk/i)).toBeTruthy())
    await waitFor(() => expect(screen.getByText(/blake3:deadbeefcafe/i)).toBeTruthy())
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
    fireEvent.input(screen.getByRole('textbox', { name: /^url$/i }), {
      target: { value: 'https://acme.example/pricing' },
    })
    fireEvent.click(screen.getByRole('button', { name: /legg til kilde/i }))

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

  it('keeps the v2 ingestion sidebar menu available on the v3 shell', () => {
    window.history.pushState(null, '', '/ingestions')

    render(() => (
      <Router root={(props) => <>{props.children}</>}>
        <Route
          path="/*all"
          component={() => (
            <CoreSidebar
              activeRoute={routeFromPath('/ingestions')}
              expanded
              onExpandedChange={vi.fn()}
              onOpenSearch={vi.fn()}
            />
          )}
        />
      </Router>
    ))

    const navigation = screen.getByRole('navigation', { name: /innhenting navigasjon/i })
    expect(within(navigation).getByRole('link', { name: /^arbeidsflate$/i }).getAttribute('href')).toBe('/ingestions')
    expect(within(navigation).getByRole('link', { name: /^kunnskap$/i }).getAttribute('href')).toBe('/knowledge')
    expect(within(navigation).getByRole('link', { name: /^spør velion$/i }).getAttribute('href')).toBe('/chat')
  })
})
