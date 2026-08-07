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
  ticketIds: ['ticket-1', 'ticket-2'],
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

  it('labels a similarity candidate result as a preview, never a shared cause, and anchors on the first evidence ticket', async () => {
    const fetchMock = vi.fn(async () => new Response(JSON.stringify({
      data: { status: 'candidate_found', candidates: [{ ticket_id: 'ticket-9' }], algorithm_version: 'v1' },
    }), { headers: { 'Content-Type': 'application/json' } }))
    vi.stubGlobal('fetch', fetchMock)

    render(() => <I18nProvider><TicketRepeatSignals orgId="org-demo" signals={signals} /></I18nProvider>)
    fireEvent.click(screen.getByRole('button', { name: /Check similarity candidates for refund|Sjekk likhetskandidater for refund/i }))

    expect(await screen.findByText(/Similarity candidate \(preview\)|Likhetskandidat \(forhåndsvisning\)/i)).toBeTruthy()
    expect(screen.getByText(/ticket-9/)).toBeTruthy()
    expect(screen.getByText(/not a shared cause, incident, or problem|ikke en felles årsak, hendelse eller problem/i)).toBeTruthy()
    await waitFor(() => {
      expect(fetchMock.mock.calls.length).toBeGreaterThan(0)
      const call = fetchMock.mock.calls[0] as [string, RequestInit | undefined] | undefined
      expect(call?.[0]).toBe('/api/v1/tickets/ticket-1/support-recurrence-candidates')
      expect(new Headers(call?.[1]?.headers).get('x-verevon-org-id')).toBe('org-demo')
    })
  })

  it('reports no candidates above threshold distinctly from an unavailable check', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({
      data: { status: 'no_candidate', candidates: [] },
    }), { headers: { 'Content-Type': 'application/json' } })))

    render(() => <I18nProvider><TicketRepeatSignals orgId="org-demo" signals={signals} /></I18nProvider>)
    fireEvent.click(screen.getByRole('button', { name: /Check similarity candidates for refund|Sjekk likhetskandidater for refund/i }))

    expect(await screen.findByText(/No similarity candidates above the threshold|Ingen likhetskandidater over terskelen/i)).toBeTruthy()
  })

  it('surfaces ZDR unavailability distinctly from a missing permission', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({
      error: { code: 'zdr_recurrence_forbidden', message: 'ZDR enabled' },
    }), { status: 412, headers: { 'Content-Type': 'application/json' } })))

    render(() => <I18nProvider><TicketRepeatSignals orgId="org-demo" signals={signals} /></I18nProvider>)
    fireEvent.click(screen.getByRole('button', { name: /Check similarity candidates for refund|Sjekk likhetskandidater for refund/i }))

    expect(await screen.findByText(/Zero Data Retention enabled|Zero Data Retention aktivert/i)).toBeTruthy()
  })

  it('surfaces a missing permission distinctly from unavailability', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({
      error: { code: 'support_recurrence_permission_required', message: 'Forbidden' },
    }), { status: 403, headers: { 'Content-Type': 'application/json' } })))

    render(() => <I18nProvider><TicketRepeatSignals orgId="org-demo" signals={signals} /></I18nProvider>)
    fireEvent.click(screen.getByRole('button', { name: /Check similarity candidates for refund|Sjekk likhetskandidater for refund/i }))

    expect(await screen.findByText(/do not have permission to view similarity candidates|ikke tilgang til å se likhetskandidater/i)).toBeTruthy()
  })
})
