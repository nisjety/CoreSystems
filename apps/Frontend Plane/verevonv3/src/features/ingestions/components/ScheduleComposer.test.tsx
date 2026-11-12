// @vitest-environment jsdom

import { createRouter, memoryHistory } from '@solidjs/router'
import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import VerevonIngestionsPage from '@/features/ingestions/components/VerevonIngestionsPage'

// WS15.S0: the Schedules surface offers only what GET /api/ingestions/schedules/capabilities
// says this installation can honour. Everything is exercised through the page, so the
// composer and panel stay private to the page module.

const SCHEDULE = {
  id: 'sched_1',
  name: 'Nightly docs',
  kind: 'crawl',
  status: 'active',
  cron: '0 2 * * *',
  createdAt: '2026-10-01T00:00:00Z',
  lastRunAt: null,
  nextRunAt: null,
  target: 'https://example.com/docs',
  config: {},
}

function jsonResponse(body: unknown, status = 200) {
  return new Response(JSON.stringify(body), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}

function stubApi(capabilities: Record<string, unknown> | 'fail') {
  vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input)
    if (url.endsWith('/api/ingestions/schedules/capabilities')) {
      if (capabilities === 'fail') {
        return jsonResponse({ error: { code: 'upstream_unavailable', message: 'The upstream service is unavailable.' } }, 502)
      }
      return jsonResponse({ data: capabilities })
    }
    if (url.endsWith('/api/ingestions/runs')) return jsonResponse({ data: [] })
    if (url.endsWith('/api/ingestions/schedules')) return jsonResponse({ data: [SCHEDULE] })
    if (url.endsWith('/api/ingestions/sources')) return jsonResponse({ data: { integrations: [], quarrySources: [] } })
    if (url.endsWith('/api/ingestions/profiles')) return jsonResponse({ data: { profiles: [] } })
    return jsonResponse({ error: { code: 'not_found', message: `Unhandled ${url}` } }, 404)
  }))
}

function renderIngestions() {
  const TestRouter = createRouter({
    routes: [{ path: '/ingestions', component: VerevonIngestionsPage }],
    history: memoryHistory('/ingestions'),
    explicitLinks: true,
  })
  return render(() => <TestRouter>{(props) => <>{props.children}</>}</TestRouter>)
}

async function openSchedules() {
  renderIngestions()
  fireEvent.click(screen.getByRole('button', { name: /tidsplaner/i }))
  await waitFor(() => expect(screen.getByRole('heading', { name: /opprett tidsplan/i })).toBeTruthy())
}

function saveButton() {
  return screen.getByRole('button', { name: /lagre tidsplan/i }) as HTMLButtonElement
}

function kindSelect() {
  const selects = screen.getAllByRole('combobox') as HTMLSelectElement[]
  const match = selects.find((select) => Array.from(select.options).some((option) => option.value === 'batch'))
  if (!match) throw new Error('schedule kind select not found')
  return match
}

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
})

describe('ScheduleComposer (WS15.S0 capability gating)', () => {
  it('with no kinds enabled shows the honest notice, disables Save and hides Trigger', async () => {
    stubApi({ trigger: false, backfill: false, overlap: false, kinds: [], needsActivation: false })
    await openSchedules()

    await waitFor(() => expect(screen.getByText(/ingen tidsplantyper er aktivert/i)).toBeTruthy())
    expect(saveButton().disabled).toBe(true)

    await waitFor(() => expect(screen.getByText('Nightly docs')).toBeTruthy())
    expect(screen.queryByRole('button', { name: /utløs/i })).toBeNull()
  })

  it('offers exactly the enabled kinds and shows Trigger only when trigger is on', async () => {
    stubApi({ trigger: true, backfill: false, overlap: false, kinds: ['crawl', 'batch'], needsActivation: false })
    await openSchedules()

    await waitFor(() => expect(kindSelect().options.length).toBe(2))
    expect(Array.from(kindSelect().options).map((option) => option.value)).toEqual(['crawl', 'batch'])
    expect(screen.queryByText(/ingen tidsplantyper er aktivert/i)).toBeNull()
    expect(saveButton().disabled).toBe(false)

    await waitFor(() => expect(screen.getByRole('button', { name: /utløs/i })).toBeTruthy())
  })

  it('moves the composer onto an enabled kind when the default kind is not enabled', async () => {
    stubApi({ trigger: false, backfill: false, overlap: false, kinds: ['batch'], needsActivation: false })
    await openSchedules()

    await waitFor(() => expect(kindSelect().options.length).toBe(1))
    expect(kindSelect().value).toBe('batch')
    expect(saveButton().disabled).toBe(false)
    expect(screen.queryByRole('button', { name: /utløs/i })).toBeNull()
  })

  it('falls back to all capabilities off when the capability read fails', async () => {
    stubApi('fail')
    await openSchedules()

    await waitFor(() => expect(screen.getByText(/ingen tidsplantyper er aktivert/i)).toBeTruthy())
    expect(saveButton().disabled).toBe(true)
    await waitFor(() => expect(screen.getByText('Nightly docs')).toBeTruthy())
    expect(screen.queryByRole('button', { name: /utløs/i })).toBeNull()
  })

  it('hides Show runs unless the installation reports runs', async () => {
    stubApi({ trigger: false, backfill: false, overlap: false, kinds: ['batch'], needsActivation: false })
    await openSchedules()

    // The kind select fills only once the capability read has landed, so the check below is not vacuous.
    await waitFor(() => expect(kindSelect().options.length).toBe(1))
    await waitFor(() => expect(screen.getByText('Nightly docs')).toBeTruthy())
    expect(screen.queryByRole('button', { name: /vis kjøringer/i })).toBeNull()
  })

  it('shows Show runs when the gateway reports runs', async () => {
    stubApi({ trigger: false, backfill: false, overlap: false, kinds: ['batch'], needsActivation: false, runs: true })
    await openSchedules()

    await waitFor(() => expect(screen.getByRole('button', { name: /vis kjøringer/i })).toBeTruthy())
  })
})
