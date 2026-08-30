import { render, screen } from '@solidjs/testing-library'
import { describe, expect, it, vi } from 'vitest'
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

describe('FleetRunConsole', () => {
  it('renders N members with distinct live views and budget bar', async () => {
    const fetchSpy = vi.fn(async (url: string) => {
      if (url.includes('/budget')) {
        return { ok: true, json: async () => budget } as Response
      }
      return { ok: true, json: async () => ({ fleet }) } as Response
    })
    vi.stubGlobal('fetch', fetchSpy as unknown as typeof fetch)

    render(() => <FleetRunConsole fleetId="fleet_abc" />)

    // Fleet header
    expect(await screen.findByText(/fleet_abc/)).toBeTruthy()
    // Budget bar updates on receipt cost
    expect(await screen.findByText(/\$0\.0150/)).toBeTruthy()
    // Three distinct live views
    const liveViews = await screen.findAllByText(/live view:/)
    expect(liveViews.length).toBe(3)
    // Per-run spend (budget fetch is async - use findByText)
    expect(await screen.findByText(/run_1111:\s*\$0\.0050/)).toBeTruthy()
  })
})
