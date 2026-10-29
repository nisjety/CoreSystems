// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor, within } from '@solidjs/testing-library'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { Route, Router } from '@solidjs/router'
import { SupportVerevonComposer } from './SupportVerevonComposer'
import type { ModelContextPack } from '@/shared/context-packs/context-pack'
import { upsertChatThreadTranscript } from '@/features/chat/lib/chat-thread-history'
import { bindSupportChatThread, clearSupportChatThreads } from '@/shared/chat/support-chat-thread'
import { runAssist } from '@/features/inbox/lib/inbox-ai'
import { createDraftReplyProposal, createTicketUpdateProposal } from '@/shared/api/inbox-client'

vi.mock('@/features/inbox/lib/inbox-ai', () => ({ runAssist: vi.fn() }))
vi.mock('@/shared/api/inbox-client', () => ({
  createDraftReplyProposal: vi.fn(),
  createInternalNoteProposal: vi.fn(),
  createTicketUpdateProposal: vi.fn(),
}))

const contextPack: ModelContextPack = {
  route: '/support/ticketing',
  currentView: 'support.ticketing',
  selectedEntity: { type: 'ticket', id: 'ticket-1', label: 'TCK-001', status: 'open' },
  visibleItems: [{ type: 'ticket', id: 'ticket-1', label: 'TCK-001', status: 'open' }],
  availableActions: [],
  redactionPolicy: 'ids-and-summaries-only',
  support: { permissions: ['support.read'] },
}

afterEach(() => {
  cleanup()
  clearSupportChatThreads()
  vi.clearAllMocks()
})

describe('SupportVerevonComposer', () => {
  it('shows the permission-scoped conversation context it gives Verevon', () => {
    render(() => (
      <SupportVerevonComposer
        contextLabel="Context: TCK-001"
        contextPack={contextPack}
        conversationId="conv-1"
        messages={[
          { agent: false, from: 'Maya Solberg', body: 'I cannot verify my email.' },
          { agent: true, from: 'Support', body: 'We are looking into this.', internal: false },
        ]}
        orgId="org-1"
        userId="user-1"
      />
    ))

    expect(screen.getByRole('region', { name: /conversation context|samtalegrunnlag/i })).toBeTruthy()
    expect(screen.getByText('I cannot verify my email.')).toBeTruthy()
    expect(screen.getByText('We are looking into this.')).toBeTruthy()
    expect(screen.getByText(/permission-scoped|autoriserte samtale/i)).toBeTruthy()
    expect(screen.getByRole('button', { name: /suggest next action|foreslå neste handling/i })).toBeTruthy()
  })

  it('rehydrates the latest Chat answer for the exact support scope', async () => {
    const scope = { userId: 'user-1', orgId: 'org-1', conversationId: 'conv-1' }
    bindSupportChatThread(scope, 'chat-thread-1')
    upsertChatThreadTranscript({
      threadId: 'chat-thread-1',
      updatedAt: '2026-08-03T10:00:00.000Z',
      turns: [
        { id: 'user-1', role: 'user', content: 'Help with this case.', createdAt: '2026-08-03T09:59:00.000Z' },
        { id: 'assistant-1', role: 'assistant', content: 'The safest next step is to verify the account detail.', createdAt: '2026-08-03T10:00:00.000Z' },
      ],
    })

    render(() => (
      <Router root={(props) => <>{props.children}</>}>
        <Route path="/*all" component={() => (
          <SupportVerevonComposer
            contextLabel="Context: TCK-001"
            contextPack={contextPack}
            conversationId="conv-1"
            messages={[]}
            orgId="org-1"
            userId="user-1"
          />
        )} />
      </Router>
    ))

    expect(await screen.findByRole('region', { name: /verevon-svar|verevon answer/i })).toBeTruthy()
    expect(screen.getByText('The safest next step is to verify the account detail.')).toBeTruthy()
  })

  it('turns the suggested next action into a reviewable reply proposal', async () => {
    vi.mocked(runAssist).mockImplementation(async (_orgId, mode) => ({
      text: mode === 'ask' ? 'Recommended next step: ask the customer to confirm the registration.' : 'Please confirm that you initiated this registration.',
      sources: [],
      model: 'verevon-balance',
      supportAiMode: 'review',
      zdr: false,
    }))
    vi.mocked(createDraftReplyProposal).mockResolvedValue({} as never)

    render(() => (
      <SupportVerevonComposer
        contextLabel="Context: TCK-001"
        contextPack={contextPack}
        conversationId="conv-1"
        messages={[{ agent: false, from: 'Maya Solberg', body: 'I cannot verify my email.' }]}
        orgId="org-1"
        userId="user-1"
      />
    ))

    fireEvent.click(screen.getByRole('button', { name: /suggest next action|foreslå neste handling/i }))
    await screen.findByText('Recommended next step: ask the customer to confirm the registration.')
    expect(screen.getByText('verevon-balance')).toBeTruthy()
    expect(screen.getByText(/confidence and cost were not reported|sikkerhet og kostnad ble ikke rapportert/i)).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: /prepare customer reply|forbered kundesvar/i }))

    await waitFor(() => expect(createDraftReplyProposal).toHaveBeenCalledWith({
      conversationId: 'conv-1',
      bodyText: 'Please confirm that you initiated this registration.',
    }))
    expect(screen.getByText(/sent for review|sendt til gjennomgang/i)).toBeTruthy()
  })

  it('stages a bounded ticket update with canonical message evidence instead of mutating the ticket', async () => {
    vi.mocked(runAssist).mockResolvedValue({
      text: JSON.stringify({
        confidence: 0.82,
        reason: 'The customer is waiting for verification.',
        suggestedFields: { status: 'waiting_customer', category: 'identity' },
      }),
      sources: [],
      model: 'verevon-balance',
      supportAiMode: 'review',
      zdr: false,
    })
    vi.mocked(createTicketUpdateProposal).mockResolvedValue({} as never)

    render(() => (
      <SupportVerevonComposer
        contextLabel="Context: TCK-001"
        contextPack={contextPack}
        conversationId="conv-1"
        ticketId="ticket-1"
        messages={[{ id: 'message-1', agent: false, from: 'Maya Solberg', body: 'I cannot verify my email.' }]}
        orgId="org-1"
        userId="user-1"
      />
    ))

    fireEvent.click(screen.getByRole('button', { name: /suggest next action|foreslå neste handling/i }))
    await screen.findByRole('button', { name: /prepare ticket update|forbered saksoppdatering/i })
    fireEvent.click(screen.getByRole('button', { name: /prepare ticket update|forbered saksoppdatering/i }))

    await waitFor(() => expect(createTicketUpdateProposal).toHaveBeenCalledWith({
      conversationId: 'conv-1',
      ticketId: 'ticket-1',
      confidence: 0.82,
      reason: 'The customer is waiting for verification.',
      evidenceMessageIds: ['message-1'],
      suggestedFields: { status: 'waiting_customer', category: 'identity' },
    }))
    expect(screen.getByText(/sent for review|sendt til gjennomgang/i)).toBeTruthy()
    expect(within(screen.getByRole('status')).getByText(/identity[\s\S]*waiting_customer/i)).toBeTruthy()
    expect(screen.queryByText(/ticket updated|sak oppdatert/i)).toBeNull()
  })

  it('does not allow a durable ticket update from preview-only text without canonical message evidence', async () => {
    vi.mocked(runAssist).mockResolvedValue({
      text: 'Recommended next step: confirm the customer details.',
      sources: [],
      supportAiMode: 'review',
      zdr: false,
    })

    render(() => (
      <SupportVerevonComposer
        contextLabel="Context: TCK-001"
        contextPack={contextPack}
        conversationId="conv-1"
        ticketId="ticket-1"
        messages={[{ agent: false, from: 'Maya Solberg', body: 'I cannot verify my email.' }]}
        orgId="org-1"
        userId="user-1"
      />
    ))

    fireEvent.click(screen.getByRole('button', { name: /suggest next action|foreslå neste handling/i }))
    const update = await screen.findByRole('button', { name: /prepare ticket update|forbered saksoppdatering/i }) as HTMLButtonElement
    expect(update.disabled).toBe(true)
    expect(createTicketUpdateProposal).not.toHaveBeenCalled()
  })
})
