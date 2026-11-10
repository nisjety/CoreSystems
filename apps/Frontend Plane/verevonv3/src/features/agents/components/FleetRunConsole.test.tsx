import { cleanup, render, screen } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { I18nProvider } from '@/shared/i18n'
import { localeStorageKey } from '@/shared/i18n/locales'
import { FleetRunConsole } from './FleetRunConsole'

const fleet = {
  fleet_id: 'fleet_abc',
  status: 'running',
  max_parallel_runs: 3,
  member_run_ids: ['run_11111111', 'run_22222222', 'run_33333333'],
}
const budget = {
  fleet_id: 'fleet_abc',
  budget_usd: 1.0,
  spent_usd: 0.015,
  over_budget: false,
  per_run_usd: {
    run_11111111: 0.005,
    run_22222222: 0.005,
    run_33333333: 0.005,
  },
}

function stubFleetFetch() {
  const fetchSpy = vi.fn(async (url: string) => {
    if (url.includes('/budget')) {
      return { ok: true, json: async () => budget } as Response
    }
    return { ok: true, json: async () => ({ fleet }) } as Response
  })
  vi.stubGlobal('fetch', fetchSpy as unknown as typeof fetch)
}

describe('FleetRunConsole', () => {
  afterEach(() => {
    cleanup()
    window.localStorage.removeItem(localeStorageKey)
  })

  it('renders N members with distinct live views and budget bar', async () => {
    stubFleetFetch()

    render(() => <FleetRunConsole threadId="thread_abc" runId="run_parent" />)

    // Fleet header
    expect(await screen.findByText(/fleet_abc/)).toBeTruthy()
    // Budget bar updates on receipt cost
    expect(await screen.findByText(/\$0\.0150/)).toBeTruthy()
    // Three distinct live views
    const liveViews = await screen.findAllByText(/direkte visning:/)
    expect(liveViews.length).toBe(3)
    // Per-run spend (budget fetch is async - use findByText)
    expect(await screen.findByText(/run_1111:\s*\$0\.0050/)).toBeTruthy()
  })

  it('labels the fleet and its cancel control in Norwegian under no', async () => {
    window.localStorage.setItem(localeStorageKey, 'no')
    stubFleetFetch()

    render(() => (
      <I18nProvider>
        <FleetRunConsole threadId="thread_abc" runId="run_parent" />
      </I18nProvider>
    ))

    expect(await screen.findByText('Flåte fleet_abc')).toBeTruthy()
    expect(await screen.findByText(/3 kjøringer · maks 3 parallelle/)).toBeTruthy()
    expect(await screen.findByRole('button', { name: 'Avbryt flåten' })).toBeTruthy()
  })

  it('labels the fleet and its cancel control in English under en', async () => {
    window.localStorage.setItem(localeStorageKey, 'en')
    stubFleetFetch()

    render(() => (
      <I18nProvider>
        <FleetRunConsole threadId="thread_abc" runId="run_parent" />
      </I18nProvider>
    ))

    expect(await screen.findByText('Fleet fleet_abc')).toBeTruthy()
    expect(await screen.findByText(/3 runs · max 3 parallel/)).toBeTruthy()
    expect(await screen.findByText('Budget')).toBeTruthy()
    expect((await screen.findAllByText(/live view:/)).length).toBe(3)
    expect(await screen.findByRole('button', { name: 'Cancel fleet' })).toBeTruthy()
  })
})
