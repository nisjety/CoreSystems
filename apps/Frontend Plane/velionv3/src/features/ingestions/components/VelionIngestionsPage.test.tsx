// @vitest-environment jsdom

import { Route, Router } from '@solidjs/router'
import { cleanup, render, screen, waitFor, within } from '@solidjs/testing-library'
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

    expect(screen.getByRole('heading', { name: 'Ingestions', level: 1 })).toBeTruthy()
    expect(screen.getByText(/run crawls and extracts, inspect evidence/i)).toBeTruthy()
    expect(screen.getByRole('button', { name: /runs/i }).getAttribute('aria-pressed')).toBe('true')
    expect(screen.getByRole('button', { name: /schedules/i })).toBeTruthy()
    expect(screen.getByRole('button', { name: /sources/i })).toBeTruthy()
    expect(screen.getByRole('button', { name: /evidence/i })).toBeTruthy()
    expect(screen.getByRole('button', { name: /profiles/i })).toBeTruthy()
    expect(screen.getByRole('button', { name: /refresh/i })).toBeTruthy()
    expect(screen.getByRole('heading', { name: /start a run/i })).toBeTruthy()
    expect((screen.getByRole('combobox', { name: /run type/i }) as HTMLSelectElement).value).toBe('scrape')
    expect(screen.getByRole('textbox', { name: /target url/i })).toBeTruthy()
    expect(screen.getByRole('button', { name: /start run/i })).toBeTruthy()
    expect(screen.getByRole('heading', { name: /recent runs/i })).toBeTruthy()
    expect(screen.getByRole('link', { name: /open knowledge/i }).getAttribute('href')).toBe('/knowledge')

    await waitFor(() => expect(screen.getByText(/no durable runs yet/i)).toBeTruthy())
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

    const navigation = screen.getByRole('navigation', { name: /ingestions navigation/i })
    expect(within(navigation).getByRole('link', { name: /^workspace$/i }).getAttribute('href')).toBe('/ingestions')
    expect(within(navigation).getByRole('link', { name: /^knowledge$/i }).getAttribute('href')).toBe('/knowledge')
    expect(within(navigation).getByRole('link', { name: /^ask velion$/i }).getAttribute('href')).toBe('/chat')
  })
})
