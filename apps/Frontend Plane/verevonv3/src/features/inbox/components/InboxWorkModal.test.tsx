// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { InboxWorkModal } from './InboxWorkModal'
import type { ZammadTicket } from '@/features/inbox/lib/inbox-model'
import type { SupportTicket } from '@/shared/api/tickets-client'

const ticket: ZammadTicket = {
  id: 42,
  number: '42',
  title: 'Missing delivery',
  state: { id: 2, name: 'open' },
  created_at: '2026-08-02T10:00:00.000Z',
  updated_at: '2026-08-02T10:00:00.000Z',
}

const target: SupportTicket = {
  id: 'ticket_existing', org_id: 'org_1', conversation_id: 'conversation_existing', ticket_key: 'SUP-42',
  status: 'open', priority: 'normal', severity: 'medium', source: 'manual',
  conversation: { id: 'conversation_existing', org_id: 'org_1', inbox_id: 'inbox_1', title: 'Existing delivery case', status: 'open', priority: 'normal', channel: 'email', created_at: '2026-08-02T10:00:00.000Z', updated_at: '2026-08-02T10:00:00.000Z' },
  created_at: '2026-08-02T10:00:00.000Z', updated_at: '2026-08-02T10:00:00.000Z',
}

afterEach(cleanup)

describe('InboxWorkModal', () => {
  it('is explicit when a requested workflow has no durable operation', () => {
    render(() => (
      <InboxWorkModal
        modal={{
          type: 'work',
          title: 'Link existing ticket',
          description: 'Connect this conversation to an existing ticket.',
        }}
        onClose={vi.fn()}
        onLinkExistingTicket={async () => false}
        selectedTicket={ticket}
        ticketCandidates={[]}
      />
    ))

    expect(screen.getByRole('heading', { name: /link existing ticket/i })).toBeTruthy()
    expect(screen.getByText(/no change saved|ingen endring lagret/i)).toBeTruthy()
    expect(screen.getByText(/does not yet have a durable, verifiable backend operation|ikke en varig, verifiserbar backend-operasjon/i)).toBeTruthy()
    expect(screen.queryByRole('button', { name: /run selected|kjør valgte/i })).toBeNull()
    expect(screen.queryByText(/verevon-guided inbox actions|verevon-veiledede innbokshandlinger/i)).toBeNull()
  })

  it('requires an explicit existing-ticket selection before it calls the canonical attachment action', async () => {
    const attach = vi.fn(async () => true)
    render(() => (
      <InboxWorkModal
        modal={{ type: 'link-ticket', conversationId: 'conversation_source', title: 'Link existing ticket' }}
        onClose={vi.fn()}
        onLinkExistingTicket={attach}
        selectedTicket={ticket}
        ticketCandidates={[target]}
      />
    ))

    const confirm = screen.getByRole('button', { name: /attach conversation|knytt samtale/i }) as HTMLButtonElement
    expect(confirm.disabled).toBe(true)
    fireEvent.input(screen.getByRole('combobox', { name: /select existing ticket|velg eksisterende sak/i }), { target: { value: target.id } })
    await waitFor(() => expect(confirm.disabled).toBe(false))
    fireEvent.click(confirm)
    await waitFor(() => expect(attach).toHaveBeenCalledWith(target))
  })
})
