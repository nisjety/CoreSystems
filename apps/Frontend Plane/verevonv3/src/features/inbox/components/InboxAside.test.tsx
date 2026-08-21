// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor, within } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { InboxAside } from './InboxAside'
import type { ZammadArticle, ZammadTicket } from '@/features/inbox/lib/inbox-model'
import { clearSupportChatThreads } from '@/shared/chat/support-chat-thread'

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

const supportTicket = {
  id: 'ticket-42',
  org_id: 'org-coresystem',
  conversation_id: 'conversation-42',
  ticket_key: 'TCK-42',
  status: 'open',
  priority: 'normal',
  severity: 'medium',
  due_at: '2026-08-03T10:00:00.000Z',
  sla_state: 'risk',
  source: 'manual',
  created_at: '2026-07-18T08:00:00.000Z',
  updated_at: '2026-07-18T09:00:00.000Z',
} as const

afterEach(() => {
  cleanup()
  clearSupportChatThreads()
  window.localStorage.clear()
  window.sessionStorage.clear()
  vi.unstubAllGlobals()
})

function openConversationActivity() {
  fireEvent.click(screen.getByRole('tab', { name: /audit|revisjon/i }))
  flush()
}

function openFollowUpCalendar() {
  fireEvent.click(screen.getByRole('tab', { name: /actions|handlinger/i }))
  flush()
}

describe('InboxAside Verevon actions', () => {
  it('exposes Details, Verevon, Actions, and Audit as direct local tabs', () => {
    render(() => (
      <InboxAside
        orgId="org-coresystem"
        articles={articles}
        recent={[]}
        onSelectRecent={vi.fn()}
        onQueueDraftReply={vi.fn(async () => true)}
        onMacroExecuted={vi.fn()}
        onOpenModal={vi.fn()}
        selectedTicket={{ ...ticket, supportTicket }}
        userId="user-coresystem"
      />
    ))

    const tablist = screen.getByRole('tablist', { name: /support context|supportkontekst/i })
    expect(within(tablist).getByRole('tab', { name: /details|detaljer/i })).toBeTruthy()
    expect(within(tablist).getByRole('tab', { name: 'Verevon' })).toBeTruthy()
    expect(within(tablist).getByRole('tab', { name: /actions|handlinger/i })).toBeTruthy()
    expect(within(tablist).getByRole('tab', { name: /audit|revisjon/i })).toBeTruthy()
  })

  it('shows the selected conversation context in the Verevon rail', () => {
    render(() => (
      <InboxAside
        orgId="org-coresystem"
        articles={articles}
        recent={[]}
        onSelectRecent={vi.fn()}
        onQueueDraftReply={vi.fn(async () => true)}
        onMacroExecuted={vi.fn()}
        onOpenModal={vi.fn()}
        selectedTicket={{ ...ticket, conversationId: 'conversation-42', supportTicket }}
        userId="user-coresystem"
      />
    ))

    fireEvent.click(screen.getByRole('tab', { name: /verevon/i }))
    flush()

    expect(screen.getByRole('region', { name: /conversation context|samtalegrunnlag/i })).toBeTruthy()
    expect(screen.getByText('The package is missing and I need it tomorrow.')).toBeTruthy()
  })

  it('starts and reuses a Chat thread only for the selected support conversation', async () => {
    window.localStorage.setItem('verevon.chat.threadId', 'unrelated-thread')
    let invokeBody: Record<string, unknown> | null = null
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      if (String(input) === '/api/v1/orgs/org-coresystem') {
        return new Response(JSON.stringify({ data: { id: 'org-coresystem', metadata: {} } }), { headers: { 'Content-Type': 'application/json' } })
      }
      if (String(input) === '/api/v1/chat/invoke') {
        invokeBody = JSON.parse(String(init?.body)) as Record<string, unknown>
        return new Response(JSON.stringify({ data: { content: 'Use the verified delivery workflow.', thread_id: 'support_thread' } }), { headers: { 'Content-Type': 'application/json' } })
      }
      return new Response(JSON.stringify({ data: {} }), { headers: { 'Content-Type': 'application/json' } })
    }))
    render(() => (
      <InboxAside
        orgId="org-coresystem"
        articles={articles}
        recent={[]}
        onSelectRecent={vi.fn()}
        onQueueDraftReply={vi.fn(async () => true)}
        onMacroExecuted={vi.fn()}
        onOpenModal={vi.fn()}
        selectedTicket={{ ...ticket, conversationId: 'conversation-42', supportTicket }}
        userId="user-coresystem"
      />
    ))

    fireEvent.click(screen.getByRole('tab', { name: /verevon/i }))
    flush()
    expect(screen.queryByRole('link', { name: /open in chat|åpne i chat/i })).toBeNull()
    fireEvent.input(screen.getByRole('textbox', { name: /ask verevon a question|spør verevon et spørsmål/i }), { target: { value: 'What should I do next?' } })
    flush()
    fireEvent.click(screen.getByRole('button', { name: /send verevon question|send spørsmål til verevon/i }))

    expect(await screen.findByText('Use the verified delivery workflow.')).toBeTruthy()
    expect(invokeBody).not.toHaveProperty('thread_id', 'unrelated-thread')
    // The link carries the resolved thread id itself (Chat's own deep-link
    // resolver opens it); a support answer must never overwrite Chat's
    // unrelated active-thread pointer as a side effect of merely answering.
    const openInChat = await screen.findByRole('link', { name: /open in chat|åpne i chat/i })
    expect(openInChat.getAttribute('href')).toBe('/chat?thread_id=support_thread')
    expect(window.localStorage.getItem('verevon.chat.threadId')).toBe('unrelated-thread')
    expect(window.localStorage.getItem('verevon.chat.supportContext.v1')).toBeNull()
  })

  it('discards an in-flight Verevon answer when the organization changes with the same provider ticket id', async () => {
    let resolveInvoke: ((response: Response) => void) | undefined
    let invokeReturned = false
    const invokeResponse = new Promise<Response>((resolve) => {
      resolveInvoke = resolve
    })
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input)
      if (url.startsWith('/api/v1/orgs/')) {
        return new Response(JSON.stringify({ data: { id: url.split('/').at(-1), metadata: {} } }), { headers: { 'Content-Type': 'application/json' } })
      }
      if (url === '/api/v1/chat/invoke') {
        const response = await invokeResponse
        invokeReturned = true
        return response
      }
      return new Response(JSON.stringify({ data: {} }), { headers: { 'Content-Type': 'application/json' } })
    }))

    const [context, setContext] = createSignal<{ orgId: string; ticket: ZammadTicket }>({
      orgId: 'org-coresystem',
      ticket: { ...ticket, conversationId: 'conversation-42', supportTicket },
    })
    render(() => (
      <InboxAside
        orgId={context().orgId}
        articles={articles}
        recent={[]}
        onSelectRecent={vi.fn()}
        onQueueDraftReply={vi.fn(async () => true)}
        onMacroExecuted={vi.fn()}
        onOpenModal={vi.fn()}
        selectedTicket={context().ticket}
        userId="user-coresystem"
      />
    ))

    fireEvent.click(screen.getByRole('tab', { name: /verevon/i }))
    flush()
    fireEvent.input(screen.getByRole('textbox', { name: /ask verevon a question|spør verevon et spørsmål/i }), { target: { value: 'Question for organization A' } })
    flush()
    fireEvent.click(screen.getByRole('button', { name: /send verevon question|send spørsmål til verevon/i }))
    await waitFor(() => expect(resolveInvoke).toBeTypeOf('function'))

    setContext({
      orgId: 'org-borealis',
      ticket: { ...ticket, conversationId: 'conversation-42', supportTicket: { ...supportTicket, org_id: 'org-borealis' } },
    })
    resolveInvoke?.(new Response(JSON.stringify({ data: { content: 'Answer from the previous organization.', thread_id: 'thread-org-a' } }), { headers: { 'Content-Type': 'application/json' } }))

    await waitFor(() => expect(invokeReturned).toBe(true))
    await Promise.resolve()
    await Promise.resolve()
    expect(screen.queryByText('Answer from the previous organization.')).toBeNull()
    expect(screen.queryByRole('link', { name: /open in chat|åpne i chat/i })).toBeNull()
  })

  it('shows the canonical linked-ticket SLA risk and deadline in Inbox context', () => {
    render(() => (
      <InboxAside
        orgId="org-coresystem"
        articles={articles}
        recent={[]}
        onSelectRecent={vi.fn()}
        onQueueDraftReply={vi.fn(async () => true)}
        onMacroExecuted={vi.fn()}
        onOpenModal={vi.fn()}
        selectedTicket={{ ...ticket, supportTicket }}
        userId="user-coresystem"
      />
    ))

    expect(screen.getByText('SLA')).toBeTruthy()
    expect(screen.getByText('Risk')).toBeTruthy()
    expect(screen.getByText('SLA-frist')).toBeTruthy()
  })

  it('shows the canonical draft lease in activity without exposing another operator identity', async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      void init
      const url = String(input)
      if (url.endsWith('/api/v1/inbox/conversations/conversation-42/draft-lease')) {
        return new Response(JSON.stringify({ data: {
          org_id: 'org-coresystem', conversation_id: 'conversation-42', user_id: 'user-coresystem',
          expires_at: '2026-08-02T18:15:00.000Z', updated_at: '2026-08-02T18:14:00.000Z',
        } }), { headers: { 'Content-Type': 'application/json' } })
      }
      return new Response(JSON.stringify({ data: {} }), { headers: { 'Content-Type': 'application/json' } })
    })
    vi.stubGlobal('fetch', fetchMock)

    render(() => (
      <InboxAside
        orgId="org-coresystem"
        articles={articles}
        recent={[]}
        onSelectRecent={vi.fn()}
        onQueueDraftReply={vi.fn(async () => true)}
        onMacroExecuted={vi.fn()}
        onOpenModal={vi.fn()}
        selectedTicket={{ ...ticket, conversationId: 'conversation-42' }}
        userId="user-coresystem"
      />
    ))

    openConversationActivity()

    expect((await screen.findByText('Du skriver et utkast')).textContent).toBe('Du skriver et utkast')
    expect(screen.queryByText(/user-coresystem/i)).toBeNull()
    expect(fetchMock.mock.calls.some(([input]) => String(input).endsWith('/api/v1/inbox/conversations/conversation-42/draft-lease'))).toBe(true)
  })

  it('follows a conversation through the audited action contract and rereads the canonical preference', async () => {
    let followed = false
    let actionBody: unknown = null
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input)
      if (url.endsWith('/api/v1/inbox/conversations/conversation-42/draft-lease')) {
        return new Response(JSON.stringify({ error: { code: 'not_found', message: 'No lease' } }), { status: 404, headers: { 'Content-Type': 'application/json' } })
      }
      if (url.endsWith('/api/v1/inbox/conversations/conversation-42/follow')) {
        if (!followed) {
          return new Response(JSON.stringify({ error: { code: 'not_found', message: 'Not followed' } }), { status: 404, headers: { 'Content-Type': 'application/json' } })
        }
        return new Response(JSON.stringify({ data: {
          org_id: 'org-coresystem', conversation_id: 'conversation-42', user_id: 'user-coresystem', created_at: '2026-08-03T10:00:00.000Z',
        } }), { headers: { 'Content-Type': 'application/json' } })
      }
      if (url.endsWith('/api/v1/actions/execute')) {
        actionBody = JSON.parse(String(init?.body))
        followed = true
        return new Response(JSON.stringify({ data: {
          actionId: 'inbox.follow_conversation', runId: 'conversation_follow_42', status: 'completed', auditId: 'audit_42', eventStream: '',
        } }), { headers: { 'Content-Type': 'application/json' } })
      }
      return new Response(JSON.stringify({ data: {} }), { headers: { 'Content-Type': 'application/json' } })
    })
    vi.stubGlobal('fetch', fetchMock)

    render(() => (
      <InboxAside
        orgId="org-coresystem"
        articles={articles}
        recent={[]}
        onSelectRecent={vi.fn()}
        onQueueDraftReply={vi.fn(async () => true)}
        onMacroExecuted={vi.fn()}
        onOpenModal={vi.fn()}
        selectedTicket={{ ...ticket, conversationId: 'conversation-42' }}
        userId="user-coresystem"
      />
    ))

    openConversationActivity()
    await screen.findByText(/lagre en personlig følgepreferanse|save a personal follow preference/i)
    fireEvent.click(screen.getByRole('button', { name: /følg samtale|follow conversation/i }))

    await waitFor(() => expect(actionBody).not.toBeNull())
    // Exact shape, not toMatchObject: action-client.ts posts exactly
    // { actionId, idempotencyKey, input }, and this is the assertion that would
    // catch an extra field silently riding along to the gateway.
    expect(actionBody).toEqual({
      actionId: 'inbox.follow_conversation',
      idempotencyKey: expect.any(String),
      input: { conversationId: 'conversation-42', following: true },
    })
    await waitFor(() => expect(screen.getByRole('button', { name: /slutt å følge|unfollow/i })).toBeTruthy())
    expect(fetchMock.mock.calls.some(([request]) => String(request).endsWith('/api/v1/actions/execute'))).toBe(true)
  })

  it('records a customer feedback preference through the audited action and rereads the canonical value', async () => {
    let optedIn = false
    let actionBody: unknown = null
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input)
      if (url.endsWith('/api/v1/inbox/conversations/conversation-42/draft-lease')) {
        return new Response(JSON.stringify({ error: { code: 'not_found', message: 'No lease' } }), { status: 404, headers: { 'Content-Type': 'application/json' } })
      }
      if (url.endsWith('/api/v1/inbox/conversations/conversation-42/follow')) {
        return new Response(JSON.stringify({ error: { code: 'not_found', message: 'Not followed' } }), { status: 404, headers: { 'Content-Type': 'application/json' } })
      }
      if (url.endsWith('/api/v1/inbox/conversations/conversation-42/csat-preference')) {
        return new Response(JSON.stringify({ data: {
          org_id: 'org-coresystem', conversation_id: 'conversation-42', contact_id: 'contact-42', opted_in: optedIn,
          updated_by: 'user-coresystem', updated_at: '2026-08-03T10:00:00.000Z',
        } }), { headers: { 'Content-Type': 'application/json' } })
      }
      if (url.endsWith('/api/v1/actions/execute')) {
        actionBody = JSON.parse(String(init?.body))
        optedIn = true
        return new Response(JSON.stringify({ data: {
          actionId: 'inbox.set_csat_preference', runId: 'conversation_csat_42', status: 'completed', auditId: 'audit_csat_42', eventStream: '',
        } }), { headers: { 'Content-Type': 'application/json' } })
      }
      return new Response(JSON.stringify({ data: {} }), { headers: { 'Content-Type': 'application/json' } })
    })
    vi.stubGlobal('fetch', fetchMock)

    render(() => (
      <InboxAside
        orgId="org-coresystem"
        articles={articles}
        recent={[]}
        onSelectRecent={vi.fn()}
        onQueueDraftReply={vi.fn(async () => true)}
        onMacroExecuted={vi.fn()}
        onOpenModal={vi.fn()}
        selectedTicket={{ ...ticket, conversationId: 'conversation-42' }}
        userId="user-coresystem"
      />
    ))

    openConversationActivity()
    await screen.findByText(/ingen spørreundersøkelse blir sendt|no survey will be sent/i)
    fireEvent.click(screen.getByRole('button', { name: /registrer samtykke|record consent/i }))

    await waitFor(() => expect(actionBody).not.toBeNull())
    expect(actionBody).toEqual({
      actionId: 'inbox.set_csat_preference',
      idempotencyKey: expect.any(String),
      input: { conversationId: 'conversation-42', optedIn: true },
    })
    await waitFor(() => expect(screen.getByRole('button', { name: /trekk tilbake samtykke|withdraw consent/i })).toBeTruthy())
    expect(fetchMock.mock.calls.filter(([request]) => String(request).endsWith('/api/v1/inbox/conversations/conversation-42/csat-preference'))).toHaveLength(2)
  })

  it('reconciles a false-failure 502 by checking whether the follow preference was actually recorded', async () => {
    let followed = false
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input)
      if (url.endsWith('/api/v1/inbox/conversations/conversation-42/draft-lease')) {
        return new Response(JSON.stringify({ error: { code: 'not_found', message: 'No lease' } }), { status: 404, headers: { 'Content-Type': 'application/json' } })
      }
      if (url.endsWith('/api/v1/inbox/conversations/conversation-42/follow')) {
        if (!followed) {
          return new Response(JSON.stringify({ error: { code: 'not_found', message: 'Not followed' } }), { status: 404, headers: { 'Content-Type': 'application/json' } })
        }
        return new Response(JSON.stringify({ data: {
          org_id: 'org-coresystem', conversation_id: 'conversation-42', user_id: 'user-coresystem', created_at: '2026-08-03T10:00:00.000Z',
        } }), { headers: { 'Content-Type': 'application/json' } })
      }
      if (url.endsWith('/api/v1/actions/execute')) {
        // The follow action reaches the backend and is durably recorded, but
        // the response itself is lost to a transient gateway error.
        followed = true
        return new Response(JSON.stringify({ message: 'Bad Gateway' }), { status: 502, headers: { 'Content-Type': 'application/json' } })
      }
      return new Response(JSON.stringify({ data: {} }), { headers: { 'Content-Type': 'application/json' } })
    })
    vi.stubGlobal('fetch', fetchMock)

    render(() => (
      <InboxAside
        orgId="org-coresystem"
        articles={articles}
        recent={[]}
        onSelectRecent={vi.fn()}
        onQueueDraftReply={vi.fn(async () => true)}
        onMacroExecuted={vi.fn()}
        onOpenModal={vi.fn()}
        selectedTicket={{ ...ticket, conversationId: 'conversation-42' }}
        userId="user-coresystem"
      />
    ))

    openConversationActivity()
    await screen.findByText(/lagre en personlig følgepreferanse|save a personal follow preference/i)
    fireEvent.click(screen.getByRole('button', { name: /følg samtale|follow conversation/i }))

    await waitFor(() => expect(screen.getByRole('button', { name: /slutt å følge|unfollow/i })).toBeTruthy())
    expect(screen.queryByText(/kunne ikke oppdateres|could not be updated/i)).toBeNull()
  })

  it('shows a real failure when following a conversation genuinely did not go through', async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input)
      if (url.endsWith('/api/v1/inbox/conversations/conversation-42/draft-lease')) {
        return new Response(JSON.stringify({ error: { code: 'not_found', message: 'No lease' } }), { status: 404, headers: { 'Content-Type': 'application/json' } })
      }
      if (url.endsWith('/api/v1/inbox/conversations/conversation-42/follow')) {
        // Still not followed — the action genuinely didn't land.
        return new Response(JSON.stringify({ error: { code: 'not_found', message: 'Not followed' } }), { status: 404, headers: { 'Content-Type': 'application/json' } })
      }
      if (url.endsWith('/api/v1/actions/execute')) {
        return new Response(JSON.stringify({ message: 'Bad Gateway' }), { status: 502, headers: { 'Content-Type': 'application/json' } })
      }
      return new Response(JSON.stringify({ data: {} }), { headers: { 'Content-Type': 'application/json' } })
    })
    vi.stubGlobal('fetch', fetchMock)

    render(() => (
      <InboxAside
        orgId="org-coresystem"
        articles={articles}
        recent={[]}
        onSelectRecent={vi.fn()}
        onQueueDraftReply={vi.fn(async () => true)}
        onMacroExecuted={vi.fn()}
        onOpenModal={vi.fn()}
        selectedTicket={{ ...ticket, conversationId: 'conversation-42' }}
        userId="user-coresystem"
      />
    ))

    openConversationActivity()
    await screen.findByText(/lagre en personlig følgepreferanse|save a personal follow preference/i)
    fireEvent.click(screen.getByRole('button', { name: /følg samtale|follow conversation/i }))

    expect(await screen.findByText(/kunne ikke oppdateres|could not be updated/i)).toBeTruthy()
    expect(screen.queryByRole('button', { name: /slutt å følge|unfollow/i })).toBeNull()
  })

  it('reconciles a false-failure 502 by checking whether the feedback preference was actually recorded', async () => {
    let optedIn = false
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input)
      if (url.endsWith('/api/v1/inbox/conversations/conversation-42/draft-lease')) {
        return new Response(JSON.stringify({ error: { code: 'not_found', message: 'No lease' } }), { status: 404, headers: { 'Content-Type': 'application/json' } })
      }
      if (url.endsWith('/api/v1/inbox/conversations/conversation-42/follow')) {
        return new Response(JSON.stringify({ error: { code: 'not_found', message: 'Not followed' } }), { status: 404, headers: { 'Content-Type': 'application/json' } })
      }
      if (url.endsWith('/api/v1/inbox/conversations/conversation-42/csat-preference')) {
        return new Response(JSON.stringify({ data: {
          org_id: 'org-coresystem', conversation_id: 'conversation-42', contact_id: 'contact-42', opted_in: optedIn,
          updated_by: 'user-coresystem', updated_at: '2026-08-03T10:00:00.000Z',
        } }), { headers: { 'Content-Type': 'application/json' } })
      }
      if (url.endsWith('/api/v1/actions/execute')) {
        // The preference reaches the backend and is durably recorded, but
        // the response itself is lost to a transient gateway error.
        optedIn = true
        return new Response(JSON.stringify({ message: 'Bad Gateway' }), { status: 502, headers: { 'Content-Type': 'application/json' } })
      }
      return new Response(JSON.stringify({ data: {} }), { headers: { 'Content-Type': 'application/json' } })
    })
    vi.stubGlobal('fetch', fetchMock)

    render(() => (
      <InboxAside
        orgId="org-coresystem"
        articles={articles}
        recent={[]}
        onSelectRecent={vi.fn()}
        onQueueDraftReply={vi.fn(async () => true)}
        onMacroExecuted={vi.fn()}
        onOpenModal={vi.fn()}
        selectedTicket={{ ...ticket, conversationId: 'conversation-42' }}
        userId="user-coresystem"
      />
    ))

    openConversationActivity()
    await screen.findByText(/ingen spørreundersøkelse blir sendt|no survey will be sent/i)
    fireEvent.click(screen.getByRole('button', { name: /registrer samtykke|record consent/i }))

    await waitFor(() => expect(screen.getByRole('button', { name: /trekk tilbake samtykke|withdraw consent/i })).toBeTruthy())
    expect(screen.queryByText(/kunne ikke oppdateres|could not be updated/i)).toBeNull()
  })

  it('shows a real failure when recording a feedback preference genuinely did not go through', async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input)
      if (url.endsWith('/api/v1/inbox/conversations/conversation-42/draft-lease')) {
        return new Response(JSON.stringify({ error: { code: 'not_found', message: 'No lease' } }), { status: 404, headers: { 'Content-Type': 'application/json' } })
      }
      if (url.endsWith('/api/v1/inbox/conversations/conversation-42/follow')) {
        return new Response(JSON.stringify({ error: { code: 'not_found', message: 'Not followed' } }), { status: 404, headers: { 'Content-Type': 'application/json' } })
      }
      if (url.endsWith('/api/v1/inbox/conversations/conversation-42/csat-preference')) {
        // Still not opted in — the preference genuinely didn't land.
        return new Response(JSON.stringify({ data: {
          org_id: 'org-coresystem', conversation_id: 'conversation-42', contact_id: 'contact-42', opted_in: false,
          updated_by: 'user-coresystem', updated_at: '2026-08-03T10:00:00.000Z',
        } }), { headers: { 'Content-Type': 'application/json' } })
      }
      if (url.endsWith('/api/v1/actions/execute')) {
        return new Response(JSON.stringify({ message: 'Bad Gateway' }), { status: 502, headers: { 'Content-Type': 'application/json' } })
      }
      return new Response(JSON.stringify({ data: {} }), { headers: { 'Content-Type': 'application/json' } })
    })
    vi.stubGlobal('fetch', fetchMock)

    render(() => (
      <InboxAside
        orgId="org-coresystem"
        articles={articles}
        recent={[]}
        onSelectRecent={vi.fn()}
        onQueueDraftReply={vi.fn(async () => true)}
        onMacroExecuted={vi.fn()}
        onOpenModal={vi.fn()}
        selectedTicket={{ ...ticket, conversationId: 'conversation-42' }}
        userId="user-coresystem"
      />
    ))

    openConversationActivity()
    await screen.findByText(/ingen spørreundersøkelse blir sendt|no survey will be sent/i)
    fireEvent.click(screen.getByRole('button', { name: /registrer samtykke|record consent/i }))

    expect(await screen.findByText(/kunne ikke oppdateres|could not be updated/i)).toBeTruthy()
    expect(screen.queryByRole('button', { name: /trekk tilbake samtykke|withdraw consent/i })).toBeNull()
  })

  it('records a consented resolved-ticket CSAT outcome through the audited action without claiming survey delivery', async () => {
    let recordedScore: number | null = null
    let actionBody: unknown = null
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input)
      if (url.endsWith('/api/v1/inbox/conversations/conversation-42/draft-lease')) {
        return new Response(JSON.stringify({ error: { code: 'not_found', message: 'No lease' } }), { status: 404, headers: { 'Content-Type': 'application/json' } })
      }
      if (url.endsWith('/api/v1/inbox/conversations/conversation-42/follow')) {
        return new Response(JSON.stringify({ error: { code: 'not_found', message: 'Not followed' } }), { status: 404, headers: { 'Content-Type': 'application/json' } })
      }
      if (url.endsWith('/api/v1/inbox/conversations/conversation-42/csat-preference')) {
        return new Response(JSON.stringify({ data: { org_id: 'org-coresystem', conversation_id: 'conversation-42', contact_id: 'contact-42', opted_in: true } }), { headers: { 'Content-Type': 'application/json' } })
      }
      if (url.endsWith('/api/v1/tickets/ticket-42/csat-outcome')) {
        if (recordedScore === null) return new Response(JSON.stringify({ error: { code: 'not_found', message: 'No score' } }), { status: 404, headers: { 'Content-Type': 'application/json' } })
        return new Response(JSON.stringify({ data: { org_id: 'org-coresystem', ticket_id: 'ticket-42', conversation_id: 'conversation-42', score: recordedScore } }), { headers: { 'Content-Type': 'application/json' } })
      }
      if (url.endsWith('/api/v1/tickets/csat-scorecard')) {
        return new Response(JSON.stringify({ data: { rated_tickets: recordedScore === null ? 0 : 1, positive_ratings: recordedScore !== null && recordedScore >= 4 ? 1 : 0 } }), { headers: { 'Content-Type': 'application/json' } })
      }
      if (url.endsWith('/api/v1/actions/execute')) {
        actionBody = JSON.parse(String(init?.body))
        recordedScore = 5
        return new Response(JSON.stringify({ data: { actionId: 'tickets.record_csat_outcome', runId: 'ticket_csat_outcome_42', status: 'completed', auditId: 'audit_csat_42', eventStream: '' } }), { headers: { 'Content-Type': 'application/json' } })
      }
      return new Response(JSON.stringify({ data: {} }), { headers: { 'Content-Type': 'application/json' } })
    })
    vi.stubGlobal('fetch', fetchMock)

    render(() => (
      <InboxAside
        orgId="org-coresystem"
        articles={articles}
        recent={[]}
        onSelectRecent={vi.fn()}
        onQueueDraftReply={vi.fn(async () => true)}
        onMacroExecuted={vi.fn()}
        onOpenModal={vi.fn()}
        selectedTicket={{ ...ticket, conversationId: 'conversation-42', supportTicket: { ...supportTicket, status: 'resolved' } }}
        userId="user-coresystem"
      />
    ))

    openConversationActivity()
    await screen.findByText(/no customer rating has been recorded|ingen kundevurdering er registrert/i)
    fireEvent.click(screen.getByRole('button', { name: '5' }))
    flush()
    fireEvent.click(screen.getByRole('button', { name: /record rating|registrer vurdering/i }))

    await waitFor(() => expect(actionBody).toEqual({
      actionId: 'tickets.record_csat_outcome',
      idempotencyKey: expect.any(String),
      input: { ticketId: 'ticket-42', score: 5 },
    }))
    expect(await screen.findByText(/customer rating recorded: 5\/5|kunden ga 5\/5/i)).toBeTruthy()
    expect(screen.queryByText(/survey sent|undersøkelse sendt/i)).toBeNull()
  })

  it('reports unavailable draft state without falsely claiming no one is drafting', async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL, _init?: RequestInit) => {
      void _init
      if (String(input).endsWith('/api/v1/inbox/conversations/conversation-42/draft-lease')) {
        return new Response(JSON.stringify({ error: { code: 'service_unavailable', message: 'Unavailable' } }), {
          headers: { 'Content-Type': 'application/json' }, status: 503,
        })
      }
      return new Response(JSON.stringify({ data: {} }), { headers: { 'Content-Type': 'application/json' } })
    })
    vi.stubGlobal('fetch', fetchMock)

    render(() => (
      <InboxAside
        orgId="org-coresystem"
        articles={articles}
        recent={[]}
        onSelectRecent={vi.fn()}
        onQueueDraftReply={vi.fn(async () => true)}
        onMacroExecuted={vi.fn()}
        onOpenModal={vi.fn()}
        selectedTicket={{ ...ticket, conversationId: 'conversation-42' }}
        userId="user-coresystem"
      />
    ))

    openConversationActivity()

    expect((await screen.findByText('Utkaststatus ikke tilgjengelig')).textContent).toBe('Utkaststatus ikke tilgjengelig')
    expect(screen.queryByText('Ingen aktiv utkastleie')).toBeNull()
  })

  it('turns a bounded model triage result into a reviewable classification proposal', async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      void init
      if (String(input) === '/api/v1/chat/invoke') {
        return new Response(JSON.stringify({ data: {
          content: '{"confidence":0.84,"reason":"The customer reports a missing delivery and needs carrier follow-up.","suggestedFields":{"category":"delivery","intent":"missing_delivery","priority":"high","severity":"medium","team_id":"team_delivery","team_name":"Delivery"}}',
          sources: [{ title: 'Delivery escalation policy', uri: 'https://kb.example.test/delivery', excerpt: 'Escalate packages missing before the promised date.' }],
        } }), {
          headers: { 'Content-Type': 'application/json' },
          status: 200,
        })
      }
      return new Response(JSON.stringify({ data: {} }), { headers: { 'Content-Type': 'application/json' } })
    })
    vi.stubGlobal('fetch', fetchMock)

    render(() => (
      <InboxAside
        orgId="org-coresystem"
        articles={articles}
        recent={[]}
        onSelectRecent={vi.fn()}
        onQueueDraftReply={vi.fn(async () => true)}
        onMacroExecuted={vi.fn()}
        onOpenModal={vi.fn()}
        filter={{ activeTab: 'open', assigned: 'unassigned', channel: 'email', label: 'Unassigned email' }}
        selectedTicket={{ ...ticket, conversationId: 'conversation-42' }}
		ticketTeams={[{ id: 'team_delivery', org_id: 'org-coresystem', name: 'Delivery', active: true, created_at: '', updated_at: '' }]}
        userId="user-coresystem"
        visibleTickets={[
          { ...ticket, conversationId: 'conversation-42' },
          { ...ticket, id: 43, number: '43', title: 'Private customer title', conversationId: 'conversation-43' },
        ]}
      />
    ))

    fireEvent.click(screen.getByRole('tab', { name: 'Verevon' }))
    flush()
    const action = screen.getByText('Foreslå triage').closest('.verevon-inbox-action-suggestion')
    expect(action).toBeTruthy()
    fireEvent.click(within(action as HTMLElement).getByRole('button', { name: 'Kjør' }))

    await waitFor(() => expect(fetchMock.mock.calls.some(([input]) => String(input) === '/api/v1/chat/invoke')).toBe(true))
    const invoke = fetchMock.mock.calls.find(([input]) => String(input) === '/api/v1/chat/invoke')
    const init = invoke?.[1] as RequestInit | undefined
    const body = JSON.parse(String(init?.body)) as { content: string; zdr: boolean }

    expect(body.zdr).toBe(false)
    expect(body.content).toMatch(/reviewable support-ticket triage proposal/i)
    expect(body.content).toMatch(/return ONLY a JSON object/i)
    expect(body.content).toContain('Current view: inbox')
    expect(body.content).toContain('Selected work item: ticket conversation-42 (inbox:42; open)')
    expect(body.content).toContain('Visible work items: ticket conversation-42 (inbox:42; open), ticket conversation-43 (inbox:43; open)')
    expect(body.content).toContain('Active filters: status=open, assignment=unassigned, channel=email')
	expect(body.content).toContain('Delivery (team_id=team_delivery)')
    expect(body.content).not.toContain('maya@example.com')
    expect(body.content).not.toContain('Missing delivery (')
    const source = await screen.findByRole('link', { name: 'Delivery escalation policy' })
    expect(source.getAttribute('href')).toBe('https://kb.example.test/delivery')
    expect(await screen.findByText('Escalate packages missing before the promised date.')).toBeTruthy()
    await waitFor(() => expect(fetchMock.mock.calls.some(([input]) => String(input) === '/api/v1/actions/execute')).toBe(true))
    const actionCall = fetchMock.mock.calls.find(([input]) => String(input) === '/api/v1/actions/execute')
    expect(JSON.parse(String(actionCall?.[1]?.body))).toMatchObject({
      actionId: 'tickets.classify_conversation',
      idempotencyKey: expect.any(String),
      input: {
        conversationId: 'conversation-42',
        confidence: 0.84,
        reason: 'The customer reports a missing delivery and needs carrier follow-up.',
		suggestedFields: { category: 'delivery', intent: 'missing_delivery', priority: 'high', severity: 'medium', team_id: 'team_delivery', team_name: 'Delivery' },
        evidenceMessageIds: ['100'],
      },
    })
  })

  it('turns a bounded model triage result into a reviewable update for an existing ticket', async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      void init
      if (String(input) === '/api/v1/chat/invoke') {
        return new Response(JSON.stringify({ data: {
          content: '{"confidence":0.84,"reason":"The delivery is now urgent.","suggestedFields":{"category":"delivery","intent":"missing_delivery","priority":"urgent","severity":"high","team_id":"team_delivery","team_name":"Delivery"}}',
        } }), { headers: { 'Content-Type': 'application/json' }, status: 200 })
      }
      if (String(input) === '/api/v1/inbox/ai-actions') {
        return new Response(JSON.stringify({ data: { id: 'update-1' } }), { headers: { 'Content-Type': 'application/json' }, status: 200 })
      }
      return new Response(JSON.stringify({ data: {} }), { headers: { 'Content-Type': 'application/json' } })
    })
    vi.stubGlobal('fetch', fetchMock)

    render(() => (
      <InboxAside
        orgId="org-coresystem"
        articles={articles}
        recent={[]}
        onSelectRecent={vi.fn()}
        onQueueDraftReply={vi.fn(async () => true)}
        onMacroExecuted={vi.fn()}
        onOpenModal={vi.fn()}
        selectedTicket={{ ...ticket, conversationId: 'conversation-42', supportTicket }}
		ticketTeams={[{ id: 'team_delivery', org_id: 'org-coresystem', name: 'Delivery', active: true, created_at: '', updated_at: '' }]}
        userId="user-coresystem"
      />
    ))

    fireEvent.click(screen.getByRole('tab', { name: 'Verevon' }))
    flush()
    const triageCard = screen.getByText('Foreslå triage').closest('.verevon-inbox-action-suggestion')
    if (!triageCard) throw new Error('Expected triage card')
    fireEvent.click(within(triageCard as HTMLElement).getByRole('button', { name: 'Kjør' }))

    await waitFor(() => expect(fetchMock.mock.calls.some(([input]) => String(input) === '/api/v1/inbox/ai-actions')).toBe(true))
    const proposalCall = fetchMock.mock.calls.find(([input]) => String(input) === '/api/v1/inbox/ai-actions')
    expect(JSON.parse(String(proposalCall?.[1]?.body))).toEqual({
      conversation_id: 'conversation-42',
      ticket_id: 'ticket-42',
      kind: 'ticket.update',
      confidence: 0.84,
      reason: 'The delivery is now urgent.',
      evidence_message_ids: ['100'],
      suggested_fields: { category: 'delivery', intent: 'missing_delivery', priority: 'urgent', severity: 'high', team_id: 'team_delivery', team_name: 'Delivery' },
    })
    expect(await screen.findByText(/AI-forslag til saksoppdatering opprettet/i)).toBeTruthy()
  })

  it('creates separately reviewable ticket-classification and incident proposals for incident triage', async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL, _init?: RequestInit) => {
      void _init
      if (String(input) === '/api/v1/chat/invoke') {
        return new Response(JSON.stringify({ data: {
          content: '{"confidence":0.91,"reason":"Multiple customers cannot complete checkout.","suggestedFields":{"work_type":"incident","priority":"urgent","severity":"critical"},"incident":{"title":"Checkout failures","customer_impact":"Customers cannot complete checkout."}}',
        } }), { headers: { 'Content-Type': 'application/json' }, status: 200 })
      }
      if (String(input) === '/api/v1/inbox/ai-actions') {
        return new Response(JSON.stringify({ data: { id: 'proposal-1' } }), { headers: { 'Content-Type': 'application/json' }, status: 201 })
      }
      return new Response(JSON.stringify({ data: {} }), { headers: { 'Content-Type': 'application/json' } })
    })
    vi.stubGlobal('fetch', fetchMock)

    render(() => (
      <InboxAside
        orgId="org-coresystem"
        articles={articles}
        recent={[]}
        onSelectRecent={vi.fn()}
        onQueueDraftReply={vi.fn(async () => true)}
        onMacroExecuted={vi.fn()}
        onOpenModal={vi.fn()}
        selectedTicket={{ ...ticket, conversationId: 'conversation-42', supportTicket }}
        userId="user-coresystem"
      />
    ))

    fireEvent.click(screen.getByRole('tab', { name: 'Verevon' }))
    flush()
    const triageCard = screen.getByText('Foreslå triage').closest('.verevon-inbox-action-suggestion')
    if (!triageCard) throw new Error('Expected triage card')
    fireEvent.click(within(triageCard as HTMLElement).getByRole('button', { name: 'Kjør' }))

    await waitFor(() => expect(fetchMock.mock.calls.filter(([input]) => String(input) === '/api/v1/inbox/ai-actions')).toHaveLength(2))
    const proposalBodies = fetchMock.mock.calls
      .filter(([input]) => String(input) === '/api/v1/inbox/ai-actions')
      .map(([, init]) => JSON.parse(String((init as RequestInit).body)))
    expect(proposalBodies).toEqual(expect.arrayContaining([
      expect.objectContaining({ kind: 'ticket.update', ticket_id: 'ticket-42', conversation_id: 'conversation-42' }),
      expect.objectContaining({
        conversation_id: 'conversation-42',
        ticket_id: 'ticket-42',
        title: 'Checkout failures',
        severity: 'critical',
        customer_impact: 'Customers cannot complete checkout.',
        confidence: 0.91,
        reason: 'Multiple customers cannot complete checkout.',
        evidence_message_ids: ['100'],
        kind: 'incident.create',
      }),
    ]))
    expect(await screen.findByText(/saksoppdatering, hendelse opprettet/i)).toBeTruthy()
  })

  it('stops sequential proposal writes when the selected conversation changes', async () => {
    let resolveFirstProposal: ((response: Response) => void) | undefined
    let firstProposalReturned = false
    const firstProposalResponse = new Promise<Response>((resolve) => {
      resolveFirstProposal = resolve
    })
    const fetchMock = vi.fn(async (input: RequestInfo | URL, _init?: RequestInit) => {
      void _init
      if (String(input) === '/api/v1/chat/invoke') {
        return new Response(JSON.stringify({ data: {
          content: '{"confidence":0.91,"reason":"Multiple customers cannot complete checkout.","suggestedFields":{"work_type":"incident","priority":"urgent","severity":"critical"},"incident":{"title":"Checkout failures","customer_impact":"Customers cannot complete checkout."},"problem":{"title":"Checkout dependency instability","summary":"Several failures share a timeout."}}',
        } }), { headers: { 'Content-Type': 'application/json' } })
      }
      if (String(input) === '/api/v1/inbox/ai-actions') {
        const response = await firstProposalResponse
        firstProposalReturned = true
        return response
      }
      return new Response(JSON.stringify({ data: {} }), { headers: { 'Content-Type': 'application/json' } })
    })
    vi.stubGlobal('fetch', fetchMock)
    const [selection, setSelection] = createSignal<{ ticket: ZammadTicket; articles: ZammadArticle[] }>({
      ticket: { ...ticket, conversationId: 'conversation-42', supportTicket },
      articles,
    })

    render(() => (
      <InboxAside
        orgId="org-coresystem"
        articles={selection().articles}
        recent={[]}
        onSelectRecent={vi.fn()}
        onQueueDraftReply={vi.fn(async () => true)}
        onMacroExecuted={vi.fn()}
        onOpenModal={vi.fn()}
        selectedTicket={selection().ticket}
        userId="user-coresystem"
      />
    ))

    fireEvent.click(screen.getByRole('tab', { name: 'Verevon' }))
    flush()
    const triageCard = screen.getByText('Foreslå triage').closest('.verevon-inbox-action-suggestion')
    if (!triageCard) throw new Error('Expected triage card')
    fireEvent.click(within(triageCard as HTMLElement).getByRole('button', { name: 'Kjør' }))
    await waitFor(() => expect(fetchMock.mock.calls.filter(([input]) => String(input) === '/api/v1/inbox/ai-actions')).toHaveLength(1))

    setSelection({
      ticket: { ...ticket, conversationId: 'conversation-43', supportTicket },
      articles: [{
        id: 200,
        ticket_id: 43,
        sender: 'Customer',
        from: 'Another customer',
        bodyText: 'A different customer conversation.',
        created_at: '2026-07-18T10:00:00.000Z',
      }],
    })
    resolveFirstProposal?.(new Response(JSON.stringify({ data: { id: 'proposal-ticket-update' } }), { headers: { 'Content-Type': 'application/json' } }))

    await waitFor(() => expect(firstProposalReturned).toBe(true))
    await Promise.resolve()
    await Promise.resolve()
    const proposalCalls = fetchMock.mock.calls.filter(([input]) => String(input) === '/api/v1/inbox/ai-actions')
    expect(proposalCalls).toHaveLength(1)
    expect(JSON.parse(String(proposalCalls[0]?.[1]?.body))).toMatchObject({
      conversation_id: 'conversation-42',
      evidence_message_ids: ['100'],
      kind: 'ticket.update',
    })
  })

  it('stages a root-cause candidate as a third independent review item for incident triage', async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL, _init?: RequestInit) => {
      void _init
      if (String(input) === '/api/v1/chat/invoke') {
        return new Response(JSON.stringify({ data: {
          content: '{"confidence":0.91,"reason":"Multiple customers cannot complete checkout.","suggestedFields":{"work_type":"incident","priority":"urgent","severity":"critical"},"incident":{"title":"Checkout failures","customer_impact":"Customers cannot complete checkout."},"problem":{"title":"Checkout dependency instability","summary":"Several checkout failures share a timeout.","root_cause":"Gateway timeout observed."}}',
        } }), { headers: { 'Content-Type': 'application/json' }, status: 200 })
      }
      if (String(input) === '/api/v1/inbox/ai-actions') {
        return new Response(JSON.stringify({ data: { id: 'proposal-1' } }), { headers: { 'Content-Type': 'application/json' }, status: 201 })
      }
      return new Response(JSON.stringify({ data: {} }), { headers: { 'Content-Type': 'application/json' } })
    })
    vi.stubGlobal('fetch', fetchMock)

    render(() => (
      <InboxAside
        orgId="org-coresystem"
        articles={articles}
        recent={[]}
        onSelectRecent={vi.fn()}
        onQueueDraftReply={vi.fn(async () => true)}
        onMacroExecuted={vi.fn()}
        onOpenModal={vi.fn()}
        selectedTicket={{ ...ticket, conversationId: 'conversation-42', supportTicket }}
        userId="user-coresystem"
      />
    ))

    fireEvent.click(screen.getByRole('tab', { name: 'Verevon' }))
    flush()
    const triageCard = screen.getByText('Foreslå triage').closest('.verevon-inbox-action-suggestion')
    if (!triageCard) throw new Error('Expected triage card')
    fireEvent.click(within(triageCard as HTMLElement).getByRole('button', { name: 'Kjør' }))

    await waitFor(() => expect(fetchMock.mock.calls.filter(([input]) => String(input) === '/api/v1/inbox/ai-actions')).toHaveLength(3))
    const proposalBodies = fetchMock.mock.calls
      .filter(([input]) => String(input) === '/api/v1/inbox/ai-actions')
      .map(([, init]) => JSON.parse(String((init as RequestInit).body)))
    expect(proposalBodies).toContainEqual(expect.objectContaining({
      kind: 'problem.create', conversation_id: 'conversation-42', title: 'Checkout dependency instability',
      summary: 'Several checkout failures share a timeout.', root_cause: 'Gateway timeout observed.',
      confidence: 0.91, reason: 'Multiple customers cannot complete checkout.', evidence_message_ids: ['100'],
    }))
    expect(await screen.findByText(/saksoppdatering, hendelse og problem opprettet/i)).toBeTruthy()
  })

  it('clears prior evidence when the latest Verevon output has no external source', async () => {
    let modelCall = 0
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      if (String(input) !== '/api/v1/chat/invoke') {
        return new Response(JSON.stringify({ data: {} }), { headers: { 'Content-Type': 'application/json' } })
      }
      modelCall += 1
      const content = modelCall === 1 ? {
        content: 'Escalate this case.',
        sources: [{ title: 'First source', uri: 'https://kb.example.test/first' }],
      } : { content: 'Customer reports a missing package.' }
      return new Response(JSON.stringify({ data: content }), { headers: { 'Content-Type': 'application/json' } })
    })
    vi.stubGlobal('fetch', fetchMock)

    render(() => (
      <InboxAside
        orgId="org-coresystem"
        articles={articles}
        recent={[]}
        onSelectRecent={vi.fn()}
        onQueueDraftReply={vi.fn(async () => true)}
        onMacroExecuted={vi.fn()}
        onOpenModal={vi.fn()}
        selectedTicket={{ ...ticket, conversationId: 'conversation-42' }}
        userId="user-coresystem"
      />
    ))

    fireEvent.click(screen.getByRole('tab', { name: 'Verevon' }))
    flush()
    const routeCard = screen.getByText('Foreslå triage').closest('.verevon-inbox-action-suggestion')
    if (!routeCard) throw new Error('Expected routing card')
    fireEvent.click(within(routeCard as HTMLElement).getByRole('button', { name: 'Kjør' }))
    await screen.findByRole('link', { name: 'First source' })

    fireEvent.click(screen.getByRole('button', { name: 'Oppsummer' }))
    expect(await screen.findByText(/siste Verevon-svaret hadde ingen eksterne kilder/i)).toBeTruthy()
    expect(screen.queryByRole('link', { name: 'First source' })).toBeNull()
  })

  it('keeps a valid triage result transient when the organization is in ZDR mode', async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      if (String(input) === '/api/v1/orgs/org-coresystem') {
        return new Response(JSON.stringify({ data: { metadata: { interactiveRetention: { zdr: true } } } }), { headers: { 'Content-Type': 'application/json' } })
      }
      if (String(input) === '/api/v1/chat/invoke') {
        return new Response(JSON.stringify({ data: {
          content: '{"confidence":0.84,"reason":"The customer reports a missing delivery.","suggestedFields":{"category":"delivery","priority":"high"}}',
        } }), { headers: { 'Content-Type': 'application/json' } })
      }
      return new Response(JSON.stringify({ data: {} }), { headers: { 'Content-Type': 'application/json' } })
    })
    vi.stubGlobal('fetch', fetchMock)

    render(() => (
      <InboxAside
        orgId="org-coresystem"
        articles={articles}
        recent={[]}
        onSelectRecent={vi.fn()}
        onQueueDraftReply={vi.fn(async () => true)}
        onMacroExecuted={vi.fn()}
        onOpenModal={vi.fn()}
        selectedTicket={{ ...ticket, conversationId: 'conversation-42' }}
        userId="user-coresystem"
      />
    ))

    fireEvent.click(screen.getByRole('tab', { name: 'Verevon' }))
    flush()
    const triageCard = screen.getByText('Foreslå triage').closest('.verevon-inbox-action-suggestion')
    if (!triageCard) throw new Error('Expected triage card')
    fireEvent.click(within(triageCard as HTMLElement).getByRole('button', { name: 'Kjør' }))

    expect(await screen.findByText(/ZDR er aktiv: triage-forslaget/i)).toBeTruthy()
    expect(fetchMock.mock.calls.some(([input]) => String(input) === '/api/v1/actions/execute')).toBe(false)
  })

  it('keeps a valid triage result transient in Support AI assist mode', async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      void init
      if (String(input).startsWith('/api/v1/orgs/')) {
        return new Response(JSON.stringify({ data: { metadata: { supportAi: { mode: 'assist' } } } }), { headers: { 'Content-Type': 'application/json' } })
      }
      if (String(input) === '/api/v1/chat/invoke') {
        return new Response(JSON.stringify({ data: {
          content: '{"confidence":0.84,"reason":"The customer reports a missing delivery and needs carrier follow-up.","suggestedFields":{"category":"delivery","intent":"missing_delivery","priority":"high","severity":"medium"}}',
        } }), { headers: { 'Content-Type': 'application/json' } })
      }
      return new Response(JSON.stringify({ data: {} }), { headers: { 'Content-Type': 'application/json' } })
    })
    vi.stubGlobal('fetch', fetchMock)

    render(() => (
      <InboxAside
        orgId="org-coresystem"
        articles={articles}
        recent={[]}
        onSelectRecent={vi.fn()}
        onQueueDraftReply={vi.fn(async () => true)}
        onMacroExecuted={vi.fn()}
        onOpenModal={vi.fn()}
        selectedTicket={{ ...ticket, conversationId: 'conversation-42' }}
        userId="user-coresystem"
      />
    ))

    fireEvent.click(screen.getByRole('tab', { name: 'Verevon' }))
    flush()
    const triageCard = screen.getByText('Foreslå triage').closest('.verevon-inbox-action-suggestion')
    if (!triageCard) throw new Error('Expected triage card')
    fireEvent.click(within(triageCard as HTMLElement).getByRole('button', { name: 'Kjør' }))

    expect(await screen.findByText(/Assistentmodus er aktiv: triage-forslaget/i)).toBeTruthy()
    expect(fetchMock.mock.calls.some(([input]) => String(input) === '/api/v1/actions/execute')).toBe(false)
    expect(fetchMock.mock.calls.some(([input]) => String(input) === '/api/v1/inbox/ai-actions')).toBe(false)
  })

  it('prepares a bounded resolution plan without creating any proposal until the operator stages an item', async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      void init
      if (String(input) === '/api/v1/orgs/org-coresystem') {
        return new Response(JSON.stringify({ data: {
          metadata: { interactiveRetention: { zdr: false }, supportAi: { mode: 'assist' } },
        } }), { headers: { 'Content-Type': 'application/json' } })
      }
      if (String(input) === '/api/v1/chat/invoke') {
        return new Response(JSON.stringify({ data: {
          content: JSON.stringify({
            summary: 'The carrier scan is overdue and needs investigation.',
            reply: 'Thanks for letting us know. We are checking the carrier scan.',
            internal_note: 'Check the carrier exception before promising a date.',
            triage: {
              confidence: 0.82,
              reason: 'The overdue carrier scan supports a delivery investigation.',
              suggestedFields: { category: 'delivery', intent: 'missing_delivery', priority: 'high', severity: 'medium' },
            },
          }),
        } }), { headers: { 'Content-Type': 'application/json' } })
      }
      return new Response(JSON.stringify({ data: {} }), { headers: { 'Content-Type': 'application/json' } })
    })
    vi.stubGlobal('fetch', fetchMock)
    const onQueueDraftReply = vi.fn(async () => true)
    const onQueueInternalNote = vi.fn(async () => true)

    render(() => (
      <InboxAside
        orgId="org-coresystem"
        articles={articles}
        recent={[]}
        onSelectRecent={vi.fn()}
        onQueueDraftReply={onQueueDraftReply}
        onQueueInternalNote={onQueueInternalNote}
        onMacroExecuted={vi.fn()}
        onOpenModal={vi.fn()}
        selectedTicket={{ ...ticket, conversationId: 'conversation-42' }}
        userId="user-coresystem"
      />
    ))

    fireEvent.click(screen.getByRole('tab', { name: 'Verevon' }))
    flush()
    const planCard = screen.getByText('Lag løsningsplan').closest('.verevon-inbox-action-suggestion')
    if (!planCard) throw new Error('Expected resolution plan card')
    fireEvent.click(within(planCard as HTMLElement).getByRole('button', { name: 'Forbered' }))

    expect(await screen.findByText('Foreslått løsningsplan')).toBeTruthy()
    expect(screen.getByText('The carrier scan is overdue and needs investigation.')).toBeTruthy()
    expect(fetchMock.mock.calls.some(([input]) => String(input) === '/api/v1/actions/execute')).toBe(false)
    expect(fetchMock.mock.calls.some(([input]) => String(input) === '/api/v1/inbox/ai-actions')).toBe(false)

    fireEvent.click(screen.getAllByRole('button', { name: 'Legg i svarhjelp' })[0] as HTMLButtonElement)
    expect((await screen.findAllByText('Thanks for letting us know. We are checking the carrier scan.')).length).toBe(1)
    fireEvent.click(screen.getByRole('button', { name: 'Sett inn utkast' }))
    await waitFor(() => expect(onQueueDraftReply).toHaveBeenCalledWith(
      'Thanks for letting us know. We are checking the carrier scan.',
      false,
      'assist',
      expect.stringMatching(/^resolution_[A-Za-z0-9_-]+$/),
    ))

    const note = screen.getByText('Check the carrier exception before promising a date.')
    const noteCard = note.closest('.verevon-inbox-field-stack')
    if (!noteCard) throw new Error('Expected internal-note plan card')
    fireEvent.click(within(noteCard as HTMLElement).getByRole('button', { name: 'Legg i svarhjelp' }))
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Sett inn utkast' }))
    await waitFor(() => expect(onQueueInternalNote).toHaveBeenCalledWith(
      'Check the carrier exception before promising a date.',
      false,
      'assist',
      expect.stringMatching(/^resolution_[A-Za-z0-9_-]+$/),
    ))
    expect(fetchMock.mock.calls.some(([input]) => String(input) === '/api/v1/inbox/ai-actions')).toBe(false)
  })

  it('keeps a source-backed reply in Assist mode instead of offering it for review', async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      if (String(input) === '/api/v1/orgs/org-coresystem') {
        return new Response(JSON.stringify({ data: {
          metadata: { interactiveRetention: { zdr: false }, supportAi: { mode: 'assist' } },
        } }), { headers: { 'Content-Type': 'application/json' } })
      }
      if (String(input) === '/api/v1/chat/invoke') {
        return new Response(JSON.stringify({ data: {
          content: 'We are checking the carrier scan before confirming a delivery date.',
        } }), { headers: { 'Content-Type': 'application/json' } })
      }
      return new Response(JSON.stringify({ data: {} }), { headers: { 'Content-Type': 'application/json' } })
    })
    vi.stubGlobal('fetch', fetchMock)
    const onQueueDraftReply = vi.fn(async () => true)

    render(() => (
      <InboxAside
        orgId="org-coresystem"
        articles={articles}
        recent={[]}
        onSelectRecent={vi.fn()}
        onQueueDraftReply={onQueueDraftReply}
        onMacroExecuted={vi.fn()}
        onOpenModal={vi.fn()}
        selectedTicket={{ ...ticket, conversationId: 'conversation-42', supportTicket }}
        userId="user-coresystem"
      />
    ))

    fireEvent.click(screen.getByRole('tab', { name: 'Verevon' }))
    flush()
    const sourceCard = screen.getByText('Kildebasert svar').closest('.verevon-inbox-action-suggestion')
    if (!sourceCard) throw new Error('Expected source-backed reply card')
    fireEvent.click(within(sourceCard as HTMLElement).getByRole('button', { name: 'Kjør' }))

    expect(await screen.findByText('We are checking the carrier scan before confirming a delivery date.')).toBeTruthy()
    const insert = screen.getByRole('button', { name: 'Sett inn utkast' })
    expect(insert).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Send til gjennomgang' })).toBeNull()
    fireEvent.click(insert)
    await waitFor(() => expect(onQueueDraftReply).toHaveBeenCalledWith(
      'We are checking the carrier scan before confirming a delivery date.',
      false,
      'assist',
      undefined,
    ))
    expect(fetchMock.mock.calls.some(([input]) => String(input) === '/api/v1/inbox/ai-actions')).toBe(false)
  })

  it('runs an Inbox macro through the audited ticket action and returns the canonical ticket', async () => {
    const macro = {
      id: 'macro-route', org_id: 'org-coresystem', name: 'Route billing', active: true,
      visibility: 'team', actions: { team_name: 'Billing' }, conditions: {},
      created_at: '2026-08-02T10:00:00.000Z', updated_at: '2026-08-02T10:00:00.000Z',
    }
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      void init
      const url = String(input)
      if (url === '/api/v1/ticket-macros') return new Response(JSON.stringify({ data: [macro] }), { headers: { 'Content-Type': 'application/json' } })
      if (url === '/api/v1/actions/execute') return new Response(JSON.stringify({ data: { actionId: 'tickets.run_macro', runId: 'run-1', status: 'completed', auditId: 'audit-1', eventStream: '' } }), { headers: { 'Content-Type': 'application/json' } })
      if (url === '/api/v1/tickets/ticket-42') return new Response(JSON.stringify({ data: { ...supportTicket, team_name: 'Billing' } }), { headers: { 'Content-Type': 'application/json' } })
      return new Response(JSON.stringify({ data: { content: '' } }), { headers: { 'Content-Type': 'application/json' } })
    })
    const onMacroExecuted = vi.fn()
    vi.stubGlobal('fetch', fetchMock)

    render(() => (
      <InboxAside
        orgId="org-coresystem"
        articles={articles}
        recent={[]}
        onSelectRecent={vi.fn()}
        onQueueDraftReply={vi.fn(async () => true)}
        onMacroExecuted={onMacroExecuted}
        onOpenModal={vi.fn()}
        selectedTicket={{ ...ticket, supportTicket }}
        userId="user-coresystem"
      />
    ))

    fireEvent.click(screen.getByRole('tab', { name: 'Verevon' }))
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Makroer' }))
    const macroRow = (await screen.findByText('Route billing')).closest('li')
    if (!macroRow) throw new Error('Expected the canonical macro row')
    fireEvent.click(within(macroRow).getByRole('button', { name: 'Bruk' }))
    const review = await screen.findByRole('dialog', { name: 'Bekreft makro' })
    expect(within(review).getByText('Gjennomgå makro før kjøring')).toBeTruthy()
    expect(fetchMock.mock.calls.some(([url]) => String(url) === '/api/v1/actions/execute')).toBe(false)
    fireEvent.click(within(review).getByRole('button', { name: 'Kjør makro' }))

    await waitFor(() => expect(onMacroExecuted).toHaveBeenCalledWith(expect.objectContaining({ id: 'ticket-42', team_name: 'Billing' })))
    const actionCall = fetchMock.mock.calls.find(([url]) => String(url) === '/api/v1/actions/execute')
    expect(JSON.parse(String(actionCall?.[1]?.body))).toMatchObject({
      actionId: 'tickets.run_macro',
      idempotencyKey: expect.any(String),
      input: { ticketId: 'ticket-42', macroId: 'macro-route', expectedMacroUpdatedAt: '2026-08-02T10:00:00.000Z' },
    })
  })

  it('persists a personal follow-up, then renders the re-read calendar state without changing ticket work', async () => {
    const savedEvent = {
      id: 'cal-1', title: 'Follow up on ticket #42', start: '2026-08-03T07:00:00.000Z', end: '2026-08-03T07:30:00.000Z',
      type: 'inbox-follow-up', status: 'scheduled', createdAt: '2026-08-02T10:00:00.000Z',
    }
    let calendar = { events: [] as typeof savedEvent[], notes: [] as never[] }
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input)
      if (url === '/api/v1/navbar/calendar' && init?.method === 'POST') {
        calendar = { events: [savedEvent], notes: [] }
        return new Response(JSON.stringify({ event: savedEvent }), { headers: { 'Content-Type': 'application/json' } })
      }
      if (url === '/api/v1/navbar/calendar') return new Response(JSON.stringify(calendar), { headers: { 'Content-Type': 'application/json' } })
      return new Response(JSON.stringify({ data: { content: '' } }), { headers: { 'Content-Type': 'application/json' } })
    })
    vi.stubGlobal('fetch', fetchMock)

    render(() => (
      <InboxAside
        orgId="org-coresystem"
        articles={articles}
        recent={[]}
        onSelectRecent={vi.fn()}
        onQueueDraftReply={vi.fn(async () => true)}
        onMacroExecuted={vi.fn()}
        onOpenModal={vi.fn()}
        selectedTicket={ticket}
        userId="user-coresystem"
      />
    ))

    openFollowUpCalendar()
    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith('/api/v1/navbar/calendar', expect.anything()))
    fireEvent.click(screen.getByRole('button', { name: 'Legg til' }))

    await waitFor(() => expect(screen.getAllByText('Follow up on ticket #42').length).toBeGreaterThan(0))
    const createCall = fetchMock.mock.calls.find(([, init]) => (init as RequestInit | undefined)?.method === 'POST')
    expect(JSON.parse(String(createCall?.[1]?.body))).toMatchObject({ title: 'Oppfølging av sak #42', type: 'inbox-follow-up' })
    expect(JSON.stringify(createCall?.[1]?.body)).not.toContain('Missing delivery')
    expect(screen.getByText(/endrer ikke sakens status eller ansvar/i)).toBeTruthy()
  })

  it('sends an AI internal note to review instead of the customer', async () => {
    const onQueueInternalNote = vi.fn(async () => true)
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
      if (String(input) === '/api/v1/chat/invoke') {
        return new Response(JSON.stringify({ data: { content: 'Verify the delivery exception with fulfillment before replying.' } }), { headers: { 'Content-Type': 'application/json' } })
      }
      return new Response(JSON.stringify({ data: {} }), { headers: { 'Content-Type': 'application/json' } })
    }))
    render(() => <InboxAside orgId="org-coresystem" articles={articles} recent={[]} onSelectRecent={vi.fn()} onQueueDraftReply={vi.fn(async () => true)} onQueueInternalNote={onQueueInternalNote} onMacroExecuted={vi.fn()} onOpenModal={vi.fn()} selectedTicket={{ ...ticket, conversationId: 'conversation-42' }} userId="user-coresystem" />)
    fireEvent.click(screen.getByRole('tab', { name: 'Verevon' }))
    flush()
    const noteCard = screen.getByText('Internt handlingsnotat').closest('.verevon-inbox-action-suggestion')
    if (!noteCard) throw new Error('Expected internal-note card')
    fireEvent.click(within(noteCard as HTMLElement).getByRole('button', { name: 'Utkast' }))
    expect(await screen.findByText('Verify the delivery exception with fulfillment before replying.')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Send notat til gjennomgang' }))
    await waitFor(() => expect(onQueueInternalNote).toHaveBeenCalledWith('Verify the delivery exception with fulfillment before replying.', false, 'review', undefined))
  })
})
