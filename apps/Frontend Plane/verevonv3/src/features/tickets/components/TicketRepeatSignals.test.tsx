// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { TicketRepeatSignals } from './TicketRepeatSignals'
import { I18nProvider } from '@/shared/i18n'

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
})

const signals = [{
  category: 'refund',
  intent: 'refund_follow_up',
  workType: 'customer_case',
  count: 2,
  ticketKeys: ['TCK-1', 'TCK-2'],
}]

describe('TicketRepeatSignals', () => {
  it('marks a knowledge gap only when the permission-aware check completes with no sources', async () => {
    const fetchMock = vi.fn(async () => new Response(JSON.stringify({
      data: { results: [], total: 0 },
    }), { headers: { 'Content-Type': 'application/json' } }))
    vi.stubGlobal('fetch', fetchMock)

    render(() => <I18nProvider><TicketRepeatSignals orgId="org-demo" signals={signals} /></I18nProvider>)
    fireEvent.click(screen.getByRole('button', { name: /Check Knowledge coverage for refund|Sjekk kunnskapsdekning for refund/i }))

    expect(await screen.findByText(/No authorized Knowledge source was returned|Ingen autorisert kunnskapskilde ble funnet/i)).toBeTruthy()
    expect(screen.getByText(/gap candidate.*not proof|gapkandidat.*ikke bevis/i)).toBeTruthy()
    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith('/api/v1/knowledge/search', expect.objectContaining({
      method: 'POST', body: JSON.stringify({ query: 'Support guidance for refund refund follow up', limit: 3 }),
    })))
  })

  it('keeps a failed lookup unverified instead of claiming a knowledge gap', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({
      error: { code: 'unavailable', message: 'Knowledge unavailable' },
    }), { status: 503, headers: { 'Content-Type': 'application/json' } })))

    render(() => <I18nProvider><TicketRepeatSignals orgId="org-demo" signals={signals} /></I18nProvider>)
    fireEvent.click(screen.getByRole('button', { name: /Check Knowledge coverage for refund|Sjekk kunnskapsdekning for refund/i }))

    expect(await screen.findByText(/Could not verify Knowledge coverage|Kunne ikke bekrefte kunnskapsdekning/i)).toBeTruthy()
    expect(screen.queryByText(/gap candidate.*not proof|gapkandidat.*ikke bevis/i)).toBeNull()
  })
})
