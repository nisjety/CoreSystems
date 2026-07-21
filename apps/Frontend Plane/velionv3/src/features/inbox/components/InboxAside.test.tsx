// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor, within } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { InboxAside } from './InboxAside'
import type { ZammadArticle, ZammadTicket } from '@/features/inbox/lib/inbox-model'

const ticket: ZammadTicket = {
  id: 42,
  number: '42',
  title: 'Missing delivery',
  customer: {
    id: 7,
    firstname: 'Maya',
    lastname: 'Solberg',
    email: 'maya@example.com',
  },
  created_at: '2026-07-18T08:00:00.000Z',
  updated_at: '2026-07-18T09:00:00.000Z',
}

const articles: ZammadArticle[] = [{
  id: 100,
  ticket_id: 42,
  sender: 'Customer',
  from: 'Maya Solberg',
  bodyText: 'The package is missing and I need it tomorrow.',
  created_at: '2026-07-18T08:00:00.000Z',
}]

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
})

describe('InboxAside Velion actions', () => {
  it('asks the model to assess escalation and routing when Assess & route runs', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      new Response(JSON.stringify({ data: { content: 'Escalate to delivery support.' } }), {
        headers: { 'Content-Type': 'application/json' },
        status: 200,
      }),
    )
    vi.stubGlobal('fetch', fetchMock)

    render(() => (
      <InboxAside
        orgId="org-aquatiq"
        articles={articles}
        recent={[]}
        onSelectRecent={vi.fn()}
        onInsertQuickReply={vi.fn()}
        onMacroExecuted={vi.fn()}
        onOpenModal={vi.fn()}
        selectedTicket={ticket}
      />
    ))

    fireEvent.click(screen.getByRole('button', { name: 'Velion' }))
    const action = screen.getByText('Assess & route').closest('.velion-inbox-action-suggestion')
    expect(action).toBeTruthy()
    fireEvent.click(within(action as HTMLElement).getByRole('button', { name: 'Run' }))

    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(1))
    const init = fetchMock.mock.calls[0]?.[1] as RequestInit | undefined
    const body = JSON.parse(String(init?.body)) as { content: string; zdr: boolean }

    expect(body.zdr).toBe(false)
    expect(body.content).toMatch(/(?:route|routing)/i)
    expect(body.content).toMatch(/escalat/i)
  })
})
