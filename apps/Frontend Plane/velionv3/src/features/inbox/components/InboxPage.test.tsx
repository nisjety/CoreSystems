// @vitest-environment jsdom

import { Route, Router } from '@solidjs/router'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import InboxPage from '@/features/inbox/components/InboxPage'
import {
  clearSession,
  markSessionOnboardingComplete,
  setSessionUser,
} from '@/shared/session/session-store'

const conversationSummary = {
  id: 'conv_order_missing',
  org_id: 'org-demo',
  inbox_id: 'inbox-support',
  title: 'Order marked delivered but missing',
  status: 'open',
  priority: 'high',
  channel: 'email',
  assignee_user_id: 'user-demo',
  assignee_name: 'Velion Demo',
  contact: { id: 'customer-maya', name: 'Maya Solberg', email: 'maya@example.com' },
  tags: ['delivery', 'urgent'],
  created_at: '2026-06-09T09:35:00.000Z',
  updated_at: '2026-06-12T08:58:00.000Z',
}

const conversationDetail = {
  ...conversationSummary,
  messages: [
    {
      id: 'msg-1001',
      conversation_id: conversationSummary.id,
      direction: 'inbound',
      sender_type: 'customer',
      sender_name: 'Maya Solberg',
      sender_email: 'maya@example.com',
      body_text: 'My package says it was delivered yesterday, but nothing arrived.',
      internal: false,
      occurred_at: '2026-06-12T07:46:00.000Z',
      created_at: '2026-06-12T07:46:00.000Z',
    },
  ],
}

const newOutlookConversation = {
  ...conversationSummary,
  id: 'conv_new_outlook',
  title: 'New Outlook message',
  contact: { id: 'customer-new', name: 'New Customer', email: 'new@example.com' },
  updated_at: '2026-07-19T16:30:00.000Z',
}

const submittedReply = {
  id: 'msg-1002',
  conversation_id: conversationSummary.id,
  direction: 'outbound',
  sender_type: 'agent',
  sender_name: 'Velion Demo',
  body_text: 'I am checking the delivery scan now.',
  internal: false,
  occurred_at: '2026-07-13T14:00:00.000Z',
  created_at: '2026-07-13T14:00:00.000Z',
}

let failFirstReply = false
let replyAttempts = 0
let detailGate: Promise<void> | null = null
let conversationListGate: Promise<void> | null = null
let conversationListError: Error | null = null
let conversationListResponse = [conversationSummary]

const connectedInboxConnections = [
  {
    id: 'conn-microsoft', providerKey: 'microsoft', displayName: 'Ima Fernandes Da Costa', status: 'active',
    capabilities: ['mail.read', 'teams.messages.read'], scopes: ['Mail.Read', 'ChannelMessage.Read.All'],
  },
  {
    id: 'conn-google', providerKey: 'google', displayName: 'Ima Dacosta', status: 'active',
    capabilities: ['gmail.read'], scopes: ['https://www.googleapis.com/auth/gmail.readonly'],
  },
  {
    id: 'conn-slack', providerKey: 'slack', displayName: 'integrationservice', status: 'active',
    capabilities: ['channels.history'], scopes: ['channels:history'],
  },
  {
    id: 'conn-meta', providerKey: 'meta', displayName: 'Ima DaCosta', status: 'active',
    capabilities: ['social.inbox.read', 'social.instagram.read', 'social.messenger.manage', 'social.whatsapp.manage'],
    scopes: ['instagram_basic', 'pages_messaging', 'whatsapp_business_messaging'],
  },
  {
    id: 'conn-discord', providerKey: 'discord', displayName: 'Ima Da Costa', status: 'active',
    capabilities: ['messages.read'], scopes: ['bot'],
  },
  {
    id: 'conn-x', providerKey: 'x', displayName: 'Ima Da Costa', status: 'active',
    capabilities: ['social.inbox.read'], scopes: ['dm.read'],
  },
]

function jsonResponse(data: unknown, status = 200) {
  return new Response(JSON.stringify({ data }), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}

function mockInboxGateway() {
  vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input)
    if (url.endsWith('/api/v1/auth/session')) {
      return jsonResponse({
        user: {
          id: 'user-demo',
          email: 'velion@example.com',
          name: 'Velion Demo',
          emailVerified: true,
        },
      })
    }
    if (url.endsWith('/api/v1/me/session-context')) {
      return jsonResponse({
        orgId: 'org-demo',
        orgs: [
          { id: 'org-other', name: 'Other workspace', role: 'member' },
          { id: 'org-demo', name: 'Velion', role: 'owner' },
        ],
      })
    }
    if (url.startsWith('/api/v1/inbox/conversations?')) {
      if (new URL(url, 'http://velion.local').searchParams.get('channel') === 'slack') {
        return jsonResponse([])
      }
      if (conversationListGate) await conversationListGate
      if (conversationListError) throw conversationListError
      return jsonResponse(conversationListResponse)
    }
    if (url.endsWith('/api/v1/integrations/connections')) {
      return jsonResponse({ connections: connectedInboxConnections })
    }
    if (url.endsWith('/api/v1/integrations/connections/conn-microsoft/inbox-history') && init?.method === 'POST') {
      const priorCalls = vi.mocked(fetch).mock.calls.filter(([input]) =>
        String(input).endsWith('/api/v1/integrations/connections/conn-microsoft/inbox-history'),
      ).length
      return jsonResponse({ history: { channel: 'teams', historyDays: 30 + (priorCalls * 30), queued: true } })
    }
    if (url.endsWith(`/api/v1/inbox/conversations/${conversationSummary.id}`)) {
      if (detailGate) await detailGate
      return jsonResponse(conversationDetail)
    }
    if (url.endsWith('/api/v1/inbox/inboxes')) {
      return jsonResponse([{ id: 'inbox-support', name: 'Commerce context' }])
    }
    if (url.endsWith(`/api/v1/inbox/conversations/${conversationSummary.id}/messages`) && init?.method === 'POST') {
      replyAttempts += 1
      if (failFirstReply && replyAttempts === 1) {
        return new Response(JSON.stringify({
          error: { code: 'provider_unavailable', message: 'Delivery outcome is not yet known.' },
        }), {
          headers: { 'Content-Type': 'application/json' },
          status: 502,
        })
      }
      return jsonResponse(submittedReply)
    }
    if (url.endsWith('/api/v1/social/drafts/from-inbox')) {
      return jsonResponse({
        post: {
          id: 'social_inbox_conv_order_missing',
          title: 'Follow-up from Order marked delivered but missing',
          body: 'We are turning this email conversation with Maya Solberg into a helpful product update.',
          status: 'draft',
          scheduledAt: '2026-06-17T09:30:00.000Z',
          platforms: ['linkedin', 'x'],
          source: {
            kind: 'inbox',
            label: 'Inbox ticket conv_order_missing',
            href: '/inbox?ticketId=conv_order_missing',
          },
          approval: { required: true, state: 'not_requested' },
          media: [],
          previews: [],
        },
      })
    }
    return jsonResponse({})
  }))
}

function renderInbox(path = '/inbox') {
  window.history.pushState(null, '', path)

  return render(() => (
    <Router root={(props) => <>{props.children}</>}>
      <Route path="/inbox" component={InboxPage} />
    </Router>
  ))
}

afterEach(() => {
  cleanup()
  clearSession()
  window.sessionStorage.clear()
  vi.unstubAllGlobals()
})

beforeEach(() => {
  clearSession()
  failFirstReply = false
  replyAttempts = 0
  detailGate = null
  conversationListGate = null
  conversationListError = null
  conversationListResponse = [conversationSummary]
  mockInboxGateway()
})

describe('InboxPage', () => {
  it('scopes inbox requests to the active organization instead of the first membership', async () => {
    renderInbox('/inbox')

    await waitFor(() => {
      const calls = vi.mocked(fetch).mock.calls as unknown as Array<[RequestInfo | URL, RequestInit | undefined]>
      expect(calls.some(([input]) => String(input).startsWith('/api/v1/inbox/conversations?'))).toBe(true)
    })
    const calls = vi.mocked(fetch).mock.calls as unknown as Array<[RequestInfo | URL, RequestInit | undefined]>
    const inboxCall = calls.find(([input]) => String(input).startsWith('/api/v1/inbox/conversations?'))

    expect(inboxCall).toBeTruthy()
    expect(new Headers(inboxCall?.[1]?.headers).get('x-velion-org-id')).toBe('org-demo')
  })

  it('loads a provider lane from the server instead of filtering a global 50-item window', async () => {
    renderInbox('/inbox?view=mine&channel=slack')

    await waitFor(() => {
      const calls = vi.mocked(fetch).mock.calls as unknown as Array<[RequestInfo | URL, RequestInit | undefined]>
      const inboxCall = calls.find(([input]) => String(input).startsWith('/api/v1/inbox/conversations?'))
      expect(inboxCall).toBeTruthy()
      expect(new URL(String(inboxCall?.[0]), 'http://velion.local').searchParams.get('channel')).toBe('slack')
    })
  })

  it('queues the next 30 days of Teams history every time older conversations are requested', async () => {
    renderInbox('/inbox?view=mine&channel=teams')

    const loadMore = await screen.findByRole('button', { name: /load older conversations/i })
    fireEvent.click(loadMore)

    await waitFor(() => {
      expect(vi.mocked(fetch).mock.calls.filter(([input, init]) =>
        String(input).endsWith('/api/v1/integrations/connections/conn-microsoft/inbox-history')
        && init?.method === 'POST',
      )).toHaveLength(1)
    })

    await waitFor(() => expect((loadMore as HTMLButtonElement).disabled).toBe(false))
    fireEvent.click(loadMore)
    await waitFor(() => {
      expect(vi.mocked(fetch).mock.calls.filter(([input, init]) =>
        String(input).endsWith('/api/v1/integrations/connections/conn-microsoft/inbox-history')
        && init?.method === 'POST',
      )).toHaveLength(2)
    })
  })

  it('still queues Teams history when local conversation pagination fails', async () => {
    conversationListResponse = Array.from({ length: 100 }, (_, index) => ({
      ...conversationSummary,
      id: `conv-teams-${index}`,
      title: `Teams conversation ${index}`,
      channel: 'teams',
      updated_at: `2026-06-${String(30 - (index % 20)).padStart(2, '0')}T08:58:00.000Z`,
    }))
    renderInbox('/inbox?view=mine&channel=teams')

    const loadMore = await screen.findByRole('button', { name: /load older conversations/i })
    conversationListError = new Error('local pagination unavailable')
    fireEvent.click(loadMore)

    await waitFor(() => expect(vi.mocked(fetch).mock.calls.some(([input, init]) =>
      String(input).endsWith('/api/v1/integrations/connections/conn-microsoft/inbox-history')
      && init?.method === 'POST',
    )).toBe(true))
  })

  it('refreshes the conversation list when the inbox window regains focus', async () => {
    renderInbox('/inbox?view=mine&channel=email')

    await waitFor(() => {
      expect(vi.mocked(fetch).mock.calls.filter(([input]) =>
        String(input).startsWith('/api/v1/inbox/conversations?'),
      )).toHaveLength(1)
    })

    window.dispatchEvent(new Event('focus'))

    await waitFor(() => {
      expect(vi.mocked(fetch).mock.calls.filter(([input]) =>
        String(input).startsWith('/api/v1/inbox/conversations?'),
      ).length).toBeGreaterThan(1)
    })
  })

  it('renders conversations returned by a background inbox refresh', async () => {
    renderInbox('/inbox?view=mine&channel=email')
    const ticketList = await screen.findByRole('list', { name: /tickets/i })
    expect(within(ticketList).getByRole('button', { name: /order marked delivered but missing/i })).toBeTruthy()

    conversationListResponse = [newOutlookConversation, conversationSummary]
    window.dispatchEvent(new Event('focus'))

    await waitFor(() => {
      expect(within(ticketList).getByRole('button', { name: /new outlook message/i })).toBeTruthy()
    })
  })

  it('keeps cached conversations visible during a slow or failed background refresh', async () => {
    let releaseRefresh: () => void = () => undefined
    renderInbox('/inbox?view=mine&channel=email')
    const ticketList = await screen.findByRole('list', { name: /tickets/i })
    const cachedTicket = () => within(ticketList).getByRole('button', { name: /order marked delivered but missing/i })
    expect(cachedTicket()).toBeTruthy()

    conversationListGate = new Promise<void>((resolve) => { releaseRefresh = resolve })
    conversationListError = new Error('temporary refresh failure')
    window.dispatchEvent(new Event('focus'))

    await waitFor(() => {
      expect(vi.mocked(fetch).mock.calls.filter(([input]) =>
        String(input).startsWith('/api/v1/inbox/conversations?'),
      )).toHaveLength(2)
    })
    expect(cachedTicket()).toBeTruthy()

    releaseRefresh()
    await waitFor(() => expect(cachedTicket()).toBeTruthy())
    expect(screen.queryByText(/temporary refresh failure/i)).toBeNull()
  })

  it('uses connected integration state for an empty lane without duplicating sidebar navigation', async () => {
    renderInbox('/inbox?view=mine&channel=slack')

    expect(await screen.findByText(/slack is connected/i)).toBeTruthy()
    expect(screen.queryByRole('navigation', { name: /connected sources/i })).toBeNull()

    const connectionCall = vi.mocked(fetch).mock.calls.find(([input]) =>
      String(input).endsWith('/api/v1/integrations/connections'),
    )
    expect(connectionCall).toBeTruthy()
    expect(new Headers(connectionCall?.[1]?.headers).get('x-velion-org-id')).toBe('org-demo')
  })

  it('does not send the synthetic all-channel route value to conversation core', async () => {
    renderInbox('/inbox?view=mine&channel=all')

    await waitFor(() => {
      const calls = vi.mocked(fetch).mock.calls as unknown as Array<[RequestInfo | URL, RequestInit | undefined]>
      const inboxCall = calls.find(([input]) => String(input).startsWith('/api/v1/inbox/conversations?'))
      expect(inboxCall).toBeTruthy()
      expect(new URL(String(inboxCall?.[0]), 'http://velion.local').searchParams.has('channel')).toBe(false)
    })
  })

  it('clears stale inbox data and refetches when the active organization changes', async () => {
    setSessionUser({
      id: 'user-demo',
      email: 'velion@example.com',
      name: 'Velion Demo',
      emailVerified: true,
    })
    markSessionOnboardingComplete({ id: 'org-demo', name: 'Velion', role: 'owner' })
    renderInbox('/inbox')

    await waitFor(() => {
      const calls = vi.mocked(fetch).mock.calls as unknown as Array<[RequestInfo | URL, RequestInit | undefined]>
      expect(calls.some(([input, init]) =>
        String(input).startsWith('/api/v1/inbox/conversations?') &&
        new Headers(init?.headers).get('x-velion-org-id') === 'org-demo',
      )).toBe(true)
    })

    markSessionOnboardingComplete({ id: 'org-next', name: 'Next workspace', role: 'owner' })

    await waitFor(() => {
      const calls = vi.mocked(fetch).mock.calls as unknown as Array<[RequestInfo | URL, RequestInit | undefined]>
      expect(calls.some(([input, init]) =>
        String(input).startsWith('/api/v1/inbox/conversations?') &&
        new Headers(init?.headers).get('x-velion-org-id') === 'org-next',
      )).toBe(true)
    })
  })

  it('discards an old organization detail response that resolves after a workspace switch', async () => {
    let releaseDetail: () => void = () => undefined
    detailGate = new Promise<void>((resolve) => { releaseDetail = resolve })
    setSessionUser({
      id: 'user-demo',
      email: 'velion@example.com',
      name: 'Velion Demo',
      emailVerified: true,
    })
    markSessionOnboardingComplete({ id: 'org-demo', name: 'Velion', role: 'owner' })
    renderInbox('/inbox')

    const ticketList = await screen.findByRole('list', { name: /tickets/i })
    fireEvent.click(within(ticketList).getByRole('button', { name: /order marked delivered but missing/i }))
    await waitFor(() => expect(vi.mocked(fetch).mock.calls.some(([input]) =>
      String(input).endsWith(`/api/v1/inbox/conversations/${conversationSummary.id}`),
    )).toBe(true))

    markSessionOnboardingComplete({ id: 'org-next', name: 'Next workspace', role: 'owner' })
    await waitFor(() => expect(vi.mocked(fetch).mock.calls.some(([input, init]) =>
      String(input).startsWith('/api/v1/inbox/conversations?')
      && new Headers(init?.headers).get('x-velion-org-id') === 'org-next',
    )).toBe(true))
    releaseDetail()

    await waitFor(() => expect(screen.getByText(/select a ticket to view the conversation/i)).toBeTruthy())
    expect(screen.queryByText(/my package says it was delivered yesterday/i)).toBeNull()
  })

  it('renders the v2-style inbox shell, queue controls, empty conversation state, and aside', () => {
    renderInbox()

    expect(screen.getByRole('heading', { name: 'Inbox' })).toBeTruthy()
    expect(screen.getByText('Select all')).toBeTruthy()
    expect(screen.getByRole('button', { name: /sort conversations/i })).toBeTruthy()
    expect(screen.getByText(/select a ticket to view the conversation/i)).toBeTruthy()
    expect(screen.getByRole('complementary', { name: /ai and customer context/i })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Details' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Velion' })).toBeTruthy()
  })

  it('opens a selected conversation and renders transcript plus reply composer', async () => {
    renderInbox()

    const ticketList = await screen.findByRole('list', { name: /tickets/i })
    fireEvent.click(within(ticketList).getByRole('button', { name: /order marked delivered but missing/i }))

    expect(await screen.findByRole('heading', { name: /order marked delivered but missing/i })).toBeTruthy()
    await waitFor(() => expect(screen.getByText(/my package says it was delivered yesterday/i)).toBeTruthy())
    expect(screen.getByPlaceholderText(/reply to maya solberg/i)).toBeTruthy()
    expect(screen.getAllByText('Commerce context').length).toBeGreaterThanOrEqual(1)
  })

  it('creates a social follow-up draft from the selected conversation and opens the calendar', async () => {
    renderInbox()

    const ticketList = await screen.findByRole('list', { name: /tickets/i })
    fireEvent.click(within(ticketList).getByRole('button', { name: /order marked delivered but missing/i }))

    const followUpButton = await screen.findByRole('button', { name: /create social follow-up/i })
    fireEvent.click(followUpButton)

    await waitFor(() => {
      const storedDraft = window.sessionStorage.getItem('velion.social.pendingDraft')
      expect(storedDraft).toContain('social_inbox_conv_order_missing')
      expect(window.location.pathname).toBe('/social/calendar')
      expect(window.location.search).toContain('source=inbox')
    })

    const socialCall = vi.mocked(fetch).mock.calls.find(([input]) =>
      String(input).endsWith('/api/v1/social/drafts/from-inbox'),
    )
    expect(socialCall?.[1]).toMatchObject({ method: 'POST' })
    expect(String(socialCall?.[1]?.body)).toContain('Order marked delivered but missing')
  })

  it('shows submitted language after a reply and never claims provider delivery', async () => {
    vi.stubGlobal('crypto', { randomUUID: vi.fn(() => 'manual-reply-1234567890') })
    renderInbox()

    const ticketList = await screen.findByRole('list', { name: /tickets/i })
    fireEvent.click(within(ticketList).getByRole('button', { name: /order marked delivered but missing/i }))

    const composer = await screen.findByRole('textbox', { name: /reply to maya solberg/i })
    fireEvent.input(composer, { target: { value: submittedReply.body_text } })
    fireEvent.click(screen.getByRole('button', { name: /^send$/i }))

    expect(await screen.findByText('Reply submitted.')).toBeTruthy()
    expect(screen.queryByText('Reply sent.')).toBeNull()

    const replyCall = vi.mocked(fetch).mock.calls.find(([input]) =>
      String(input).endsWith(`/api/v1/inbox/conversations/${conversationSummary.id}/messages`),
    )
    expect(JSON.parse(String(replyCall?.[1]?.body))).toMatchObject({
      body_text: submittedReply.body_text,
      idempotency_key: 'manual-reply-1234567890',
      internal: false,
    })
  })

  it('reuses the same idempotency key when an ambiguous reply is retried unchanged', async () => {
    failFirstReply = true
    const randomUUID = vi.fn(() => 'manual-reply-1234567890')
    vi.stubGlobal('crypto', { randomUUID })
    renderInbox()

    const ticketList = await screen.findByRole('list', { name: /tickets/i })
    fireEvent.click(within(ticketList).getByRole('button', { name: /order marked delivered but missing/i }))

    const composer = await screen.findByRole('textbox', { name: /reply to maya solberg/i })
    fireEvent.input(composer, { target: { value: submittedReply.body_text } })
    fireEvent.click(screen.getByRole('button', { name: /^send$/i }))

    expect(await screen.findByText('Delivery outcome is not yet known.')).toBeTruthy()
    await waitFor(() => expect((composer as HTMLTextAreaElement).value).toBe(submittedReply.body_text))

    fireEvent.click(screen.getByRole('button', { name: /^send$/i }))
    expect(await screen.findByText('Reply submitted.')).toBeTruthy()

    const replyCalls = vi.mocked(fetch).mock.calls.filter(([input]) =>
      String(input).endsWith(`/api/v1/inbox/conversations/${conversationSummary.id}/messages`),
    )
    expect(replyCalls).toHaveLength(2)
    expect(replyCalls.map(([, init]) => JSON.parse(String(init?.body)).idempotency_key)).toEqual([
      'manual-reply-1234567890',
      'manual-reply-1234567890',
    ])
    expect(randomUUID).toHaveBeenCalledTimes(1)
  })
})
