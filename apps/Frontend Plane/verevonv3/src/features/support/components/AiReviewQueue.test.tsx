// @vitest-environment jsdom

import { Route, Router } from '@solidjs/router'
import { cleanup, render, screen, waitFor, within } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { AiReviewQueue } from './AiReviewQueue'
import { I18nProvider } from '@/shared/i18n'

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
})

describe('AiReviewQueue', () => {
  it('lists only the tenant review queue and opens each proposal in its source conversation', async () => {
    window.history.pushState(null, '', '/support?surface=review')
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      if (String(input) === '/api/v1/inbox/ai-actions?status=review&limit=100') {
        return new Response(JSON.stringify({ data: [{
          id: 'aiact-1', org_id: 'org-1', conversation_id: 'conv-1', kind: 'problem.create', status: 'suggested',
          payload: { title: 'Checkout dependency instability', confidence: 0.82 }, created_by: 'agent-1',
          created_at: '2026-08-03T10:00:00.000Z', updated_at: '2026-08-03T10:00:00.000Z',
        }] }), { headers: { 'Content-Type': 'application/json' } })
      }
      if (String(input) === '/api/v1/inbox/ai-actions?status=all&limit=100') {
        return new Response(JSON.stringify({ data: [
          { id: 'aiact-2', org_id: 'org-1', conversation_id: 'conv-1', kind: 'draft.reply', status: 'executed', payload: {}, created_by: 'agent-1', created_at: '2026-08-03T09:00:00.000Z', updated_at: '2026-08-03T09:01:00.000Z' },
          { id: 'aiact-3', org_id: 'org-1', conversation_id: 'conv-2', kind: 'ticket.update', status: 'rejected', payload: {}, created_by: 'agent-1', created_at: '2026-08-03T08:00:00.000Z', updated_at: '2026-08-03T08:01:00.000Z' },
          { id: 'aiact-4', org_id: 'org-1', conversation_id: 'conv-3', kind: 'incident.create', status: 'failed', payload: {}, created_by: 'agent-1', created_at: '2026-08-03T07:00:00.000Z', updated_at: '2026-08-03T07:01:00.000Z' },
        ] }), { headers: { 'Content-Type': 'application/json' } })
      }
      return new Response(JSON.stringify({ data: [] }), { headers: { 'Content-Type': 'application/json' } })
    })
    vi.stubGlobal('fetch', fetchMock)

    render(() => (
      <I18nProvider>
        <Router root={(props) => <>{props.children}</>}>
          <Route path="/support" component={AiReviewQueue} />
        </Router>
      </I18nProvider>
    ))

    expect(await screen.findByRole('main', { name: /AI review|AI-gjennomgang/i })).toBeTruthy()
    expect(screen.getByRole('heading', { name: /Proposals that need a decision|Forslag som trenger beslutning/i })).toBeTruthy()
    expect(await screen.findByText(/Proposed Problem|Foreslått problem/i)).toBeTruthy()
    const outcomes = await screen.findByRole('region', { name: /Recent AI review outcomes|Nylige AI-gjennomgangsutfall/i })
    expect(within(outcomes).getByText(/Approved or executed|Godkjent eller utført/i).nextElementSibling?.textContent).toBe('1')
    expect(within(outcomes).getByText(/Declined|Avvist/i).nextElementSibling?.textContent).toBe('1')
    expect(within(outcomes).getByText(/Failed after approval|Feilet etter godkjenning/i).nextElementSibling?.textContent).toBe('1')
    const actionMix = await screen.findByRole('region', { name: /Proposal mix|Forslagsmiks/i })
    expect(within(actionMix).getByText(/Proposed reply|Foreslått svar/i).nextElementSibling?.textContent).toBe('1')
    expect(within(actionMix).getByText(/Proposed ticket update|Foreslått saksoppdatering/i).nextElementSibling?.textContent).toBe('1')
    expect(within(actionMix).getByText(/Proposed incident|Foreslått hendelse/i).nextElementSibling?.textContent).toBe('1')
    const timing = await screen.findByRole('region', { name: /Recorded decision timing|Tid til registrert beslutning/i })
    expect(within(timing).getByText(/Median time|Median tid/i).nextElementSibling?.textContent).toBe('1 min')
    expect(within(timing).getByText(/Measured actions|Målte handlinger/i).nextElementSibling?.textContent).toBe('3')
    expect(within(timing).getByText(/not delivery time|ikke leveringstid/i)).toBeTruthy()
    const reviewLink = screen.getByRole('link', { name: /Review in context|Gjennomgå i kontekst/i })
    expect(reviewLink.getAttribute('href')).toBe('/support?view=all&conversation_id=conv-1')
    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith('/api/v1/inbox/ai-actions?status=review&limit=100', expect.anything()))
    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith('/api/v1/inbox/ai-actions?status=all&limit=100', expect.anything()))
  })
})
