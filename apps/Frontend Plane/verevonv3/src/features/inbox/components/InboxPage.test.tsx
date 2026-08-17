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

const providerAuth = vi.hoisted(() => ({ runDirectOauthWindow: vi.fn() }))

vi.mock('@/shared/integrations/provider-auth-window', () => providerAuth)

const conversationSummary = {
  id: 'conv_order_missing',
  org_id: 'org-demo',
  inbox_id: 'inbox-support',
  title: 'Order marked delivered but missing',
  status: 'open',
  priority: 'high',
  channel: 'email',
  assignee_user_id: 'user-demo',
  assignee_name: 'Verevon Demo',
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
  sender_name: 'Verevon Demo',
  body_text: 'I am checking the delivery scan now.',
  internal: false,
  provider: 'microsoft',
  provider_message_id: 'provider-message-1002',
  occurred_at: '2026-07-13T14:00:00.000Z',
  created_at: '2026-07-13T14:00:00.000Z',
}

const createdSupportTicket = {
  id: 'ticket_new',
  org_id: 'org-demo',
  conversation_id: conversationSummary.id,
  ticket_key: 'TCK-100',
  status: 'open',
  priority: 'high',
  severity: 'high',
  category: 'delivery',
  intent: 'customer_follow_up',
  source: 'manual',
  created_at: '2026-08-02T12:00:00.000Z',
  updated_at: '2026-08-02T12:00:00.000Z',
}

let failFirstReply = false
let replyAttempts = 0
let draftLeaseConflict = false
let draftLeaseUnavailable = false
let draftLeaseGate: Promise<void> | null = null
let activeDraftLease: { org_id: string; conversation_id: string; user_id: string; expires_at: string; updated_at: string } | null = null
let personalDraft: { body_text: string; internal: boolean } | null = null
let draftPersistenceForbidden = false
let outboundIntentsResponse: unknown[] = []
let conversationActivityResponse: unknown[] = []
let detailGate: Promise<void> | null = null
let conversationListGate: Promise<void> | null = null
let conversationListError: Error | null = null
let conversationListResponse = [conversationSummary]
let inboxWorkspaceState = { pinnedConversationIds: [] as string[], readConversationIds: [] as string[] }

type InboxConnectionFixture = {
  id: string
  providerKey: string
  displayName: string
  status: string
  capabilities: string[]
  scopes: string[]
  metadata?: Record<string, unknown>
}

const connectedInboxConnections: InboxConnectionFixture[] = [
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

let inboxConnectionsResponse: InboxConnectionFixture[] = connectedInboxConnections
let inboxConnectionsStatus = 200
let inboxConnectionsErrorMessage = 'Integration authentication is temporarily unavailable.'

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
          email: 'verevon@example.com',
          name: 'Verevon Demo',
          emailVerified: true,
        },
      })
    }
    if (url.endsWith('/api/v1/me/session-context')) {
      return jsonResponse({
        orgId: 'org-demo',
        orgs: [
          { id: 'org-other', name: 'Other workspace', role: 'member' },
          { id: 'org-demo', name: 'Verevon', role: 'owner' },
        ],
      })
    }
    if (url.endsWith('/api/v1/inbox/workspace')) {
      return jsonResponse(inboxWorkspaceState)
    }
    if (url.endsWith(`/api/v1/inbox/conversations/${conversationSummary.id}/draft-lease`) && !init?.method) {
      if (!activeDraftLease) {
        return new Response(JSON.stringify({ error: { code: 'not_found', message: 'No active draft lease.' } }), {
          headers: { 'Content-Type': 'application/json' }, status: 404,
        })
      }
      return jsonResponse(activeDraftLease)
    }
    if (url.endsWith(`/api/v1/inbox/conversations/${conversationSummary.id}/draft-lease`) && init?.method === 'POST') {
      if (draftLeaseGate) await draftLeaseGate
      if (draftLeaseConflict) {
        return new Response(JSON.stringify({ error: { code: 'conflict', message: 'Another operator is drafting.' } }), {
          headers: { 'Content-Type': 'application/json' },
          status: 409,
        })
      }
      if (draftLeaseUnavailable) {
        return new Response(JSON.stringify({ error: { code: 'service_unavailable', message: 'Draft lease service is unavailable.' } }), {
          headers: { 'Content-Type': 'application/json' },
          status: 503,
        })
      }
      return jsonResponse({
        org_id: 'org-demo',
        conversation_id: conversationSummary.id,
        user_id: 'user-demo',
        expires_at: '2026-08-02T12:01:00.000Z',
        updated_at: '2026-08-02T12:00:00.000Z',
      })
    }
    if (url.endsWith(`/api/v1/inbox/conversations/${conversationSummary.id}/draft-lease`) && init?.method === 'DELETE') {
      return new Response(null, { status: 204 })
    }
    if (url.endsWith(`/api/v1/inbox/conversations/${conversationSummary.id}/draft`)) {
      if (draftPersistenceForbidden) {
        return new Response(JSON.stringify({ error: { code: 'zdr_draft_persistence_forbidden', message: 'Personal drafts are not retained.' } }), {
          headers: { 'Content-Type': 'application/json' }, status: 412,
        })
      }
      if (init?.method === 'PUT') {
        const body = JSON.parse(String(init.body)) as { body_text: string; internal: boolean }
        personalDraft = { body_text: body.body_text, internal: body.internal }
        return jsonResponse({
          org_id: 'org-demo', conversation_id: conversationSummary.id, user_id: 'user-demo',
          ...personalDraft, updated_at: '2026-08-02T12:00:00.000Z',
        })
      }
      if (init?.method === 'DELETE') {
        personalDraft = null
        return new Response(null, { status: 204 })
      }
      if (!personalDraft) {
        return new Response(JSON.stringify({ error: { code: 'not_found', message: 'No draft.' } }), {
          headers: { 'Content-Type': 'application/json' }, status: 404,
        })
      }
      return jsonResponse({
        org_id: 'org-demo', conversation_id: conversationSummary.id, user_id: 'user-demo',
        ...personalDraft, updated_at: '2026-08-02T12:00:00.000Z',
      })
    }
    if (url.endsWith(`/api/v1/inbox/conversations/${conversationSummary.id}/outbound-intents`)) {
      return jsonResponse(outboundIntentsResponse)
    }
    if (url.includes(`/api/v1/inbox/conversations/${conversationSummary.id}/activity?`)) {
      return jsonResponse(conversationActivityResponse)
    }
    if (url.endsWith('/api/v1/inbox/workspace/pins') && init?.method === 'POST') {
      const body = JSON.parse(String(init.body)) as { conversationId: string; enabled: boolean }
      inboxWorkspaceState = {
        ...inboxWorkspaceState,
        pinnedConversationIds: body.enabled
          ? [...new Set([...inboxWorkspaceState.pinnedConversationIds, body.conversationId])]
          : inboxWorkspaceState.pinnedConversationIds.filter((id) => id !== body.conversationId),
      }
      return jsonResponse(inboxWorkspaceState)
    }
    if (url.endsWith('/api/v1/inbox/workspace/read') && init?.method === 'POST') {
      const body = JSON.parse(String(init.body)) as { conversationId: string; enabled: boolean }
      inboxWorkspaceState = {
        ...inboxWorkspaceState,
        readConversationIds: body.enabled
          ? [...new Set([...inboxWorkspaceState.readConversationIds, body.conversationId])]
          : inboxWorkspaceState.readConversationIds.filter((id) => id !== body.conversationId),
      }
      return jsonResponse(inboxWorkspaceState)
    }
    if (url.startsWith('/api/v1/inbox/conversations?')) {
      if (new URL(url, 'http://verevon.local').searchParams.get('channel') === 'slack') {
        return jsonResponse([])
      }
      if (conversationListGate) await conversationListGate
      if (conversationListError) throw conversationListError
      return jsonResponse(conversationListResponse)
    }
    if (url.endsWith('/api/v1/integrations/connections/conn-google/inbox-sync') && init?.method === 'POST') {
      return jsonResponse({ syncJob: { id: 'sync-inbox-google', connectionId: 'conn-google', status: 'waiting_provider' } })
    }
    if (url.endsWith('/api/v1/integrations/connections/conn-microsoft/inbox-sync') && init?.method === 'POST') {
      return jsonResponse({ syncJob: { id: 'sync-inbox-microsoft', connectionId: 'conn-microsoft', status: 'waiting_provider' } })
    }
    if (url.endsWith('/api/v1/integrations/sync-jobs/sync-inbox-google')) {
      return jsonResponse({ syncJob: { id: 'sync-inbox-google', connectionId: 'conn-google', status: 'completed' } })
    }
    if (url.endsWith('/api/v1/integrations/sync-jobs/sync-inbox-microsoft')) {
      return jsonResponse({ syncJob: { id: 'sync-inbox-microsoft', connectionId: 'conn-microsoft', status: 'failed' } })
    }
    if (url.endsWith('/api/v1/integrations/connections')) {
      if (inboxConnectionsStatus !== 200) {
        return new Response(JSON.stringify({
          error: { code: 'integration_error', message: inboxConnectionsErrorMessage },
        }), {
          headers: { 'Content-Type': 'application/json' },
          status: inboxConnectionsStatus,
        })
      }
      return jsonResponse({ connections: inboxConnectionsResponse })
    }
    if (url.endsWith('/api/v1/integrations/providers/google/connect-session') && init?.method === 'POST') {
      return jsonResponse({
        id: 'connect_google_1', providerId: 'google', authMode: 'direct-oauth',
        connectUrl: 'https://accounts.example.test/oauth', sessionToken: 'connect_google_1',
      })
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
    if (url.endsWith('/api/v1/actions/execute') && init?.method === 'POST') {
      return jsonResponse({
        actionId: 'tickets.create',
        runId: 'run_ticket_create',
        status: 'completed',
        auditId: 'audit_ticket_create',
        eventStream: '',
        ticketId: createdSupportTicket.id,
      })
    }
    if (url.endsWith(`/api/v1/tickets/${createdSupportTicket.id}`)) {
      return jsonResponse(createdSupportTicket)
    }
    if (url.endsWith('/api/v1/inbox/inboxes')) {
      return jsonResponse([{ id: 'inbox-support', name: 'Commerce context' }])
    }
    if (url.endsWith(`/api/v1/inbox/conversations/${conversationSummary.id}/messages`) && init?.method === 'POST') {
      replyAttempts += 1
      if (failFirstReply && replyAttempts === 1) {
        return new Response(JSON.stringify({
          error: { code: 'delivery_unknown', message: 'Delivery outcome is not yet known.' },
        }), {
          headers: { 'Content-Type': 'application/json' },
          status: 409,
        })
      }
      outboundIntentsResponse = [{
        id: 'intent-1002', conversation_id: conversationSummary.id, status: 'submitted', provider: 'microsoft',
        provider_message_id: 'provider-message-1002', created_at: '2026-07-13T14:00:00.000Z', updated_at: '2026-07-13T14:00:00.000Z',
      }]
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
  draftLeaseConflict = false
  draftLeaseUnavailable = false
  draftLeaseGate = null
  activeDraftLease = null
  personalDraft = null
  draftPersistenceForbidden = false
  outboundIntentsResponse = []
  conversationActivityResponse = []
  detailGate = null
  conversationListGate = null
  conversationListError = null
  conversationListResponse = [conversationSummary]
  inboxConnectionsResponse = connectedInboxConnections
  inboxConnectionsStatus = 200
  inboxConnectionsErrorMessage = 'Integration authentication is temporarily unavailable.'
  inboxWorkspaceState = { pinnedConversationIds: [], readConversationIds: [] }
  providerAuth.runDirectOauthWindow.mockReset()
  providerAuth.runDirectOauthWindow.mockResolvedValue(undefined)
  mockInboxGateway()
})

describe('InboxPage', () => {
  it('keeps conversations usable and exposes a retry when the optional connection catalogue is unavailable', async () => {
    inboxConnectionsStatus = 503
    renderInbox('/inbox')

    expect(await screen.findByRole('button', { name: /order marked delivered but missing/i })).toBeTruthy()
    expect((await screen.findByRole('status')).textContent).toMatch(/connection status could not be loaded|tilkoblingsstatusen kunne ikke lastes/i)
    expect(screen.getByRole('status').textContent).toMatch(/integration authentication is temporarily unavailable/i)

    inboxConnectionsStatus = 200
    fireEvent.click(screen.getByRole('button', { name: /retry connection status|prøv tilkoblingsstatus på nytt/i }))

    await waitFor(() => {
      const connectionReads = vi.mocked(fetch).mock.calls.filter(([input]) => String(input).endsWith('/api/v1/integrations/connections'))
      expect(connectionReads).toHaveLength(2)
    })
    await waitFor(() => expect(screen.queryByRole('status')).toBeNull())
  })

  it('starts a scoped full Gmail inbox connection from the empty Inbox state', async () => {
    inboxConnectionsResponse = []
    conversationListResponse = []
    renderInbox('/inbox')

    fireEvent.click(await screen.findByRole('button', { name: 'Koble til Gmail' }))

    await waitFor(() => expect(vi.mocked(fetch).mock.calls.some(([input, init]) =>
      String(input).endsWith('/api/v1/integrations/providers/google/connect-session') && init?.method === 'POST',
    )).toBe(true))
    await waitFor(() => expect(providerAuth.runDirectOauthWindow).toHaveBeenCalledWith({
      connectUrl: 'https://accounts.example.test/oauth', sessionToken: 'connect_google_1',
    }))
    const connectCall = vi.mocked(fetch).mock.calls.find(([input, init]) =>
      String(input).endsWith('/api/v1/integrations/providers/google/connect-session') && init?.method === 'POST',
    )
    expect(JSON.parse(String(connectCall?.[1]?.body))).toEqual({ bundles: ['full'] })
  })

  it('opens a review-queue handoff in the original conversation from its URL', async () => {
    renderInbox(`/inbox?view=all&conversation_id=${conversationSummary.id}`)

    await waitFor(() => {
      expect(vi.mocked(fetch).mock.calls.some(([input]) =>
        String(input).endsWith(`/api/v1/inbox/conversations/${conversationSummary.id}`),
      )).toBe(true)
    })
    expect((await screen.findAllByRole('button', { name: /close conversation|lukk samtale/i })).length).toBeGreaterThan(0)
  })

  it('scopes inbox requests to the active organization instead of the first membership', async () => {
    renderInbox('/inbox')

    await waitFor(() => {
      const calls = vi.mocked(fetch).mock.calls as unknown as Array<[RequestInfo | URL, RequestInit | undefined]>
      expect(calls.some(([input]) => String(input).startsWith('/api/v1/inbox/conversations?'))).toBe(true)
    })
    const calls = vi.mocked(fetch).mock.calls as unknown as Array<[RequestInfo | URL, RequestInit | undefined]>
    const inboxCall = calls.find(([input]) => String(input).startsWith('/api/v1/inbox/conversations?'))

    expect(inboxCall).toBeTruthy()
    expect(new Headers(inboxCall?.[1]?.headers).get('x-verevon-org-id')).toBe('org-demo')
  })

  it('pins the selected conversation from its header through the durable personal workspace boundary', async () => {
    renderInbox('/inbox?view=mine&channel=email')

    const row = await screen.findByRole('button', { name: /order marked delivered but missing/i })
    fireEvent.click(row)
    const pin = await screen.findByRole('button', { name: /fest samtale i personlig visning|pin conversation to personal view/i })
    fireEvent.click(pin)

    await waitFor(() => {
      const call = vi.mocked(fetch).mock.calls.find(([input, init]) =>
        String(input).endsWith('/api/v1/inbox/workspace/pins') && init?.method === 'POST',
      )
      expect(call).toBeTruthy()
      expect(JSON.parse(String(call?.[1]?.body))).toEqual({ conversationId: conversationSummary.id, enabled: true })
    })
    expect(await screen.findByRole('button', { name: /løsne samtale fra personlig visning|unpin conversation from personal view/i })).toBeTruthy()
  })

  it('loads a provider lane from the server instead of filtering a global 50-item window', async () => {
    renderInbox('/inbox?view=mine&channel=slack')

    await waitFor(() => {
      const calls = vi.mocked(fetch).mock.calls as unknown as Array<[RequestInfo | URL, RequestInit | undefined]>
      const inboxCall = calls.find(([input]) => String(input).startsWith('/api/v1/inbox/conversations?'))
      expect(inboxCall).toBeTruthy()
      expect(new URL(String(inboxCall?.[0]), 'http://verevon.local').searchParams.get('channel')).toBe('slack')
    })
  })

  it('queues the next 30 days of Teams history every time older conversations are requested', async () => {
    renderInbox('/inbox?view=mine&channel=teams')

    const loadMore = await screen.findByRole('button', { name: /last inn eldre samtaler/i })
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

    const loadMore = await screen.findByRole('button', { name: /last inn eldre samtaler/i })
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

  it('keeps the active mailbox scope during a background refresh', async () => {
    renderInbox('/inbox?view=all&channel=email&connection_id=conn-google')

    await waitFor(() => {
      expect(vi.mocked(fetch).mock.calls.filter(([input]) =>
        String(input).startsWith('/api/v1/inbox/conversations?'),
      )).toHaveLength(1)
    })

    window.dispatchEvent(new Event('focus'))

    await waitFor(() => {
      const inboxCalls = vi.mocked(fetch).mock.calls.filter(([input]) =>
        String(input).startsWith('/api/v1/inbox/conversations?'),
      )
      expect(inboxCalls).toHaveLength(2)
      expect(inboxCalls.map(([input]) => new URL(String(input), 'http://verevon.local').searchParams.get('connection_id')))
        .toEqual(['conn-google', 'conn-google'])
    })
  })

  it('uses the active mailbox as the Inbox heading and keeps a clear control', async () => {
    renderInbox('/inbox?view=all&channel=email&connection_id=conn-google')

    expect(await screen.findByRole('heading', { name: 'Ima Dacosta' })).toBeTruthy()
    expect(screen.queryByRole('heading', { name: 'Innboks' })).toBeNull()
    expect(screen.getByRole('button', { name: /vis alle e-postkontoer|show all email accounts/i })).toBeTruthy()
  })

  it('queues the selected mailbox with the inbox worker and waits for its terminal receipt', async () => {
    renderInbox('/inbox?view=all&channel=email&connection_id=conn-google')

    fireEvent.click(await screen.findByRole('button', { name: /hent nye.*meldinger|fetch new.*messages/i }))

    await waitFor(() => {
      const request = vi.mocked(fetch).mock.calls.find(([input, init]) =>
        String(input).endsWith('/api/v1/integrations/connections/conn-google/inbox-sync') && init?.method === 'POST',
      )
      expect(request).toBeTruthy()
      expect(JSON.parse(String(request?.[1]?.body))).toEqual({ channel: 'email' })
    })
    expect(await screen.findByText(/oppdateringen fra leverandøren er fullført|provider refresh completed/i)).toBeTruthy()
  })

  it('reports a partial refresh when one active mailbox completes and another fails', async () => {
    renderInbox('/inbox?view=all&channel=email')

    fireEvent.click(await screen.findByRole('button', { name: /hent nye.*meldinger|fetch new.*messages/i }))

    expect(await screen.findByText(/noen leverandører ble oppdatert|some providers refreshed/i)).toBeTruthy()
  })

  it('renders conversations returned by a background inbox refresh', async () => {
    renderInbox('/inbox?view=mine&channel=email')
    const ticketList = await screen.findByRole('list', { name: /saker/i })
    expect(ticketList.getAttribute('aria-keyshortcuts')).toBe('j k')
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
    const ticketList = await screen.findByRole('list', { name: /saker/i })
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

    expect(await screen.findByText(/slack er tilkoblet/i)).toBeTruthy()
    expect(screen.queryByRole('navigation', { name: /connected sources/i })).toBeNull()

    const connectionCall = vi.mocked(fetch).mock.calls.find(([input]) =>
      String(input).endsWith('/api/v1/integrations/connections'),
    )
    expect(connectionCall).toBeTruthy()
    expect(new Headers(connectionCall?.[1]?.headers).get('x-verevon-org-id')).toBe('org-demo')
  })

  it('explains the Discord inbox prerequisites before a Discord source is provisioned', async () => {
    inboxConnectionsResponse = []
    conversationListResponse = []

    renderInbox('/inbox?view=all&channel=discord')

    expect(await screen.findByText(/discord inbox needs setup|discord trenger oppsett for innboks/i)).toBeTruthy()
    expect(screen.getByText(/message content intent|message content intent/i)).toBeTruthy()
    expect(screen.getByRole('link', { name: /open integrations|åpne integrasjoner/i })).toBeTruthy()
  })

  it('does not imply Discord delivery is healthy when OAuth is authorized but sync prerequisites are pending', async () => {
    inboxConnectionsResponse = [{
      id: 'conn-discord', providerKey: 'discord', displayName: 'Verevon server', status: 'active',
      capabilities: ['messages.read'], scopes: ['bot'],
    }]
    conversationListResponse = []

    renderInbox('/inbox?view=all&channel=discord')

    expect(await screen.findByText(/discord connection is authorized|discord-tilkoblingen er autorisert/i)).toBeTruthy()
    expect(screen.getByText(/discord inbox requires|discord-innboksen krever/i)).toBeTruthy()
  })

  it('describes an asset-connected Instagram lane without promising webhook delivery', async () => {
    inboxConnectionsResponse = [{
      id: 'conn-instagram', providerKey: 'instagram', displayName: 'nisjety', status: 'active',
      capabilities: ['social.inbox.read'], scopes: ['instagram_business_manage_messages'],
      metadata: { meta_instagram_account_ids: ['17841450361324032'] },
    }]
    conversationListResponse = []

    renderInbox('/inbox?view=all&channel=instagram')

    expect(await screen.findByText(/instagram account is connected|instagram-kontoen er tilkoblet/i)).toBeTruthy()
    expect(screen.getByText(/no conversations have been received|ingen samtaler er mottatt/i)).toBeTruthy()
    expect(screen.queryByText(/as soon as ingestion delivers|så snart innhentingen leverer/i)).toBeNull()
  })

  it('does not send the synthetic all-channel route value to conversation core', async () => {
    renderInbox('/inbox?view=mine&channel=all')

    await waitFor(() => {
      const calls = vi.mocked(fetch).mock.calls as unknown as Array<[RequestInfo | URL, RequestInit | undefined]>
      const inboxCall = calls.find(([input]) => String(input).startsWith('/api/v1/inbox/conversations?'))
      expect(inboxCall).toBeTruthy()
      expect(new URL(String(inboxCall?.[0]), 'http://verevon.local').searchParams.has('channel')).toBe(false)
    })
  })

  it('clears stale inbox data and refetches when the active organization changes', async () => {
    setSessionUser({
      id: 'user-demo',
      email: 'verevon@example.com',
      name: 'Verevon Demo',
      emailVerified: true,
    })
    markSessionOnboardingComplete({ id: 'org-demo', name: 'Verevon', role: 'owner' })
    renderInbox('/inbox')

    await waitFor(() => {
      const calls = vi.mocked(fetch).mock.calls as unknown as Array<[RequestInfo | URL, RequestInit | undefined]>
      expect(calls.some(([input, init]) =>
        String(input).startsWith('/api/v1/inbox/conversations?') &&
        new Headers(init?.headers).get('x-verevon-org-id') === 'org-demo',
      )).toBe(true)
    })

    markSessionOnboardingComplete({ id: 'org-next', name: 'Next workspace', role: 'owner' })

    await waitFor(() => {
      const calls = vi.mocked(fetch).mock.calls as unknown as Array<[RequestInfo | URL, RequestInit | undefined]>
      expect(calls.some(([input, init]) =>
        String(input).startsWith('/api/v1/inbox/conversations?') &&
        new Headers(init?.headers).get('x-verevon-org-id') === 'org-next',
      )).toBe(true)
    })
  })

  it('discards an old organization detail response that resolves after a workspace switch', async () => {
    let releaseDetail: () => void = () => undefined
    detailGate = new Promise<void>((resolve) => { releaseDetail = resolve })
    setSessionUser({
      id: 'user-demo',
      email: 'verevon@example.com',
      name: 'Verevon Demo',
      emailVerified: true,
    })
    markSessionOnboardingComplete({ id: 'org-demo', name: 'Verevon', role: 'owner' })
    renderInbox('/inbox')

    const ticketList = await screen.findByRole('list', { name: /saker/i })
    fireEvent.click(within(ticketList).getByRole('button', { name: /order marked delivered but missing/i }))
    await waitFor(() => expect(vi.mocked(fetch).mock.calls.some(([input]) =>
      String(input).endsWith(`/api/v1/inbox/conversations/${conversationSummary.id}`),
    )).toBe(true))

    markSessionOnboardingComplete({ id: 'org-next', name: 'Next workspace', role: 'owner' })
    await waitFor(() => expect(vi.mocked(fetch).mock.calls.some(([input, init]) =>
      String(input).startsWith('/api/v1/inbox/conversations?')
      && new Headers(init?.headers).get('x-verevon-org-id') === 'org-next',
    )).toBe(true))
    releaseDetail()

    await waitFor(() => expect(screen.getByText(/velg en sak for å se samtalen/i)).toBeTruthy())
    expect(screen.queryByText(/my package says it was delivered yesterday/i)).toBeNull()
  })

  it('renders the v2-style inbox shell, queue controls, empty conversation state, and aside', () => {
    renderInbox()

    expect(screen.getByRole('heading', { name: 'Innboks' })).toBeTruthy()
    expect(screen.queryByText('Velg alle')).toBeNull()
    expect(screen.getByRole('button', { name: /sorter samtaler/i })).toBeTruthy()
    expect(screen.getByText(/velg en sak for å se samtalen/i)).toBeTruthy()
    expect(screen.getByRole('complementary', { name: /supportkontekst og verktøy|support context and tools/i })).toBeTruthy()
    expect(screen.getByRole('tab', { name: 'Detaljer' })).toBeTruthy()
    expect(screen.getByRole('tab', { name: 'Verevon' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Åpne' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Venter' })).toBeTruthy()
		expect(screen.getByRole('searchbox', { name: /søk i samtaler/i }).getAttribute('aria-keyshortcuts')).toBe('/')
  })

	it('focuses persistent queue search with slash without stealing composer input', async () => {
		renderInbox()
		const search = screen.getByRole('searchbox', { name: /søk i samtaler/i })
		fireEvent.keyDown(window, { key: '/' })
		expect(document.activeElement).toBe(search)

		fireEvent.input(search, { target: { value: 'delivered' } })
		expect(await screen.findByRole('button', { name: /order marked delivered but missing/i })).toBeTruthy()
	})

	it('opens a selected conversation and renders transcript plus reply composer', async () => {
    renderInbox()

    const ticketList = await screen.findByRole('list', { name: /saker/i })
    fireEvent.click(within(ticketList).getByRole('button', { name: /order marked delivered but missing/i }))

    expect(await screen.findByRole('heading', { name: /order marked delivered but missing/i })).toBeTruthy()
    const centerTabs = screen.getByRole('tablist', { name: /conversation workspace|samtalearbeidsområde/i })
    expect(within(centerTabs).getByRole('tab', { name: /conversation|samtale/i })).toBeTruthy()
    expect(within(centerTabs).getByRole('tab', { name: /ticket|sak/i })).toBeTruthy()
    expect(within(centerTabs).getByRole('tab', { name: /activity|aktivitet/i })).toBeTruthy()
    expect(screen.getByRole('button', { name: /more conversation actions|flere samtalehandlinger/i })).toBeTruthy()
    await waitFor(() => expect(screen.getByText(/my package says it was delivered yesterday/i)).toBeTruthy())
    expect(screen.getByPlaceholderText(/svar til maya solberg/i)).toBeTruthy()
    expect(screen.getAllByText('Commerce context').length).toBeGreaterThanOrEqual(1)
  })

	it('renders the canonical work activity projection without exposing actor identifiers', async () => {
    conversationActivityResponse = [{
      id: 'audit_status_1',
      action: 'status.changed',
      actor_user_id: 'operator_private',
      created_at: '2026-08-03T08:00:00.000Z',
    }]
    renderInbox()

    const ticketList = await screen.findByRole('list', { name: /saker/i })
    fireEvent.click(within(ticketList).getByRole('button', { name: /order marked delivered but missing/i }))
    await screen.findByText(/my package says it was delivered yesterday/i)
    const centerTabs = await screen.findByRole('tablist', { name: /conversation workspace|samtalearbeidsområde/i })
    fireEvent.click(within(centerTabs).getByRole('tab', { name: /activity|aktivitet/i }))

    await screen.findByRole('heading', { name: /arbeidsaktivitet|work activity/i })
    expect(await screen.findByText(/samtalestatus oppdatert|conversation status updated/i)).toBeTruthy()
    expect(screen.queryByText('operator_private')).toBeNull()
  })

	it('moves to the next visible conversation with j without stealing composer input', async () => {
    conversationListResponse = [conversationSummary, newOutlookConversation]
    renderInbox()
    const ticketList = await screen.findByRole('list', { name: /saker/i })
    fireEvent.click(within(ticketList).getByRole('button', { name: /order marked delivered but missing/i }))
    await screen.findByRole('heading', { name: /order marked delivered but missing/i })

    const composer = screen.getByPlaceholderText(/svar til maya solberg/i)
    fireEvent.keyDown(composer, { key: 'j' })
    expect(vi.mocked(fetch).mock.calls.filter(([url]) => String(url).endsWith(`/api/v1/inbox/conversations/${newOutlookConversation.id}`))).toHaveLength(0)

    fireEvent.keyDown(window, { key: 'j' })
    await waitFor(() => expect(vi.mocked(fetch).mock.calls.some(([url]) => String(url).endsWith(`/api/v1/inbox/conversations/${newOutlookConversation.id}`))).toBe(true))
	})

  it('shows a canonical teammate draft lease before a second operator edits', async () => {
    activeDraftLease = {
      org_id: 'org-demo', conversation_id: conversationSummary.id, user_id: 'user-teammate',
      expires_at: '2099-01-01T00:00:00.000Z', updated_at: '2098-12-31T23:59:00.000Z',
    }
    renderInbox()
    const ticketList = await screen.findByRole('list', { name: /saker/i })
    fireEvent.click(within(ticketList).getByRole('button', { name: /order marked delivered but missing/i }))

    await screen.findByPlaceholderText(/svar til maya solberg/i)
    expect((await screen.findByRole('status')).textContent).toMatch(/another teammate|en annen kollega/i)
    const composer = screen.getByRole('textbox', { name: /svar til maya solberg/i })
    expect((composer as HTMLTextAreaElement).disabled).toBe(true)

    activeDraftLease = null
    fireEvent.click(screen.getByRole('button', { name: /utkaststatus|drafting status/i }))
    await waitFor(() => expect((composer as HTMLTextAreaElement).disabled).toBe(false))
  })

  it('creates a durable case through the audited ticket action and reloads its canonical state', async () => {
    renderInbox()

    const ticketList = await screen.findByRole('list', { name: /saker/i })
    fireEvent.click(within(ticketList).getByRole('button', { name: /order marked delivered but missing/i }))

    fireEvent.click(await screen.findByRole('button', { name: /opprett sak/i }))

    await waitFor(() => {
      const actionCall = vi.mocked(fetch).mock.calls.find(([input, init]) =>
        String(input).endsWith('/api/v1/actions/execute') && init?.method === 'POST',
      )
      expect(actionCall).toBeTruthy()
      expect(JSON.parse(String(actionCall?.[1]?.body))).toMatchObject({
        actionId: 'tickets.create',
        idempotencyKey: expect.any(String),
        input: {
          conversationId: conversationSummary.id,
          priority: 'high',
          severity: 'high',
          workType: 'customer_case',
          category: 'delivery',
          intent: 'customer_follow_up',
        },
      })
    })

    await waitFor(() => expect(vi.mocked(fetch).mock.calls.some(([input]) =>
      String(input).endsWith(`/api/v1/tickets/${createdSupportTicket.id}`),
    )).toBe(true))
  })

  it('creates a social follow-up draft from the selected conversation and opens the calendar', async () => {
    renderInbox()

    const ticketList = await screen.findByRole('list', { name: /saker/i })
    fireEvent.click(within(ticketList).getByRole('button', { name: /order marked delivered but missing/i }))

    const followUpButton = await screen.findByRole('button', { name: /opprett sosial oppfølging/i })
    fireEvent.click(followUpButton)

    await waitFor(() => {
      const storedDraft = window.sessionStorage.getItem('verevon.social.pendingDraft')
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

  it('shows a persisted provider-acceptance receipt after a reply without claiming delivery', async () => {
    vi.stubGlobal('crypto', { randomUUID: vi.fn(() => 'manual-reply-1234567890') })
    renderInbox()

    const ticketList = await screen.findByRole('list', { name: /saker/i })
    fireEvent.click(within(ticketList).getByRole('button', { name: /order marked delivered but missing/i }))

    // Wait for the canonical transcript read before simulating a send. This
    // avoids racing the initial detail response against the newly persisted
    // outbound receipt.
    await screen.findByText(/my package says it was delivered yesterday/i)
    const composer = await screen.findByRole('textbox', { name: /svar til maya solberg/i })
    fireEvent.input(composer, { target: { value: submittedReply.body_text } })
    fireEvent.click(screen.getByRole('button', { name: /^send$/i }))

    expect(await screen.findByText('Svar sendt.')).toBeTruthy()
    expect(screen.queryByText('Reply sent.')).toBeNull()
    expect(await screen.findByText(/sendt til microsoft; leverandøren har akseptert forespørselen/i)).toBeTruthy()
    expect(screen.queryByText(/levering er bekreftet/i)).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: /more conversation actions|flere samtalehandlinger/i }))
    fireEvent.click(screen.getByRole('menuitem', { name: /show delivery activity|vis leveringsaktivitet/i }))
    expect(await screen.findByText(/sendt til microsoft; leverandøren godtok forespørselen/i)).toBeTruthy()
    expect(screen.queryByText(/delivery is confirmed/i)).toBeNull()

    const replyCall = vi.mocked(fetch).mock.calls.find(([input]) =>
      String(input).endsWith(`/api/v1/inbox/conversations/${conversationSummary.id}/messages`),
    )
    expect(vi.mocked(fetch).mock.calls.find(([input, init]) =>
      String(input).endsWith(`/api/v1/inbox/conversations/${conversationSummary.id}/draft-lease`)
      && init?.method === 'POST',
    )).toBeTruthy()
    expect(JSON.parse(String(replyCall?.[1]?.body))).toMatchObject({
      body_text: submittedReply.body_text,
      idempotency_key: 'manual-reply-1234567890',
      internal: false,
    })
  })

  it('makes a reconciled unknown outcome explicit and blocks a false delivery claim', async () => {
    outboundIntentsResponse = [{
      id: 'intent-stale', conversation_id: conversationSummary.id, status: 'unknown', provider: 'microsoft',
      error_code: 'stale_sending_timeout', created_at: '2026-08-02T12:00:00.000Z', updated_at: '2026-08-02T12:15:00.000Z',
    }]
    renderInbox()

    const ticketList = await screen.findByRole('list', { name: /saker/i })
    fireEvent.click(within(ticketList).getByRole('button', { name: /order marked delivered but missing/i }))

    await screen.findByText(/my package says it was delivered yesterday/i)
    fireEvent.click(await screen.findByRole('tab', { name: /activity|aktivitet/i }))
    expect(await screen.findByText(/utfallet er ukjent.*ikke prøv automatisk på nytt/i)).toBeTruthy()
    expect(screen.queryByText(/levering er bekreftet/i)).toBeNull()
  })

  it('locks the composer when another authenticated operator owns the canonical draft lease', async () => {
    draftLeaseConflict = true
    renderInbox()

    const ticketList = await screen.findByRole('list', { name: /saker/i })
    fireEvent.click(within(ticketList).getByRole('button', { name: /order marked delivered but missing/i }))

    const composer = await screen.findByRole('textbox', { name: /svar til maya solberg/i })
    fireEvent.focus(composer)

    expect((await screen.findByRole('alert')).textContent).toMatch(/en annen operatør skriver et utkast/i)
    expect((composer as HTMLTextAreaElement).disabled).toBe(true)
    expect((screen.getByRole('button', { name: /^send$/i }) as HTMLButtonElement).disabled).toBe(true)
  })

  it('reports an unavailable draft lease honestly without falsely claiming another operator owns it', async () => {
    draftLeaseUnavailable = true
    renderInbox()

    const ticketList = await screen.findByRole('list', { name: /saker/i })
    fireEvent.click(within(ticketList).getByRole('button', { name: /order marked delivered but missing/i }))

    const composer = await screen.findByRole('textbox', { name: /svar til maya solberg/i })
    fireEvent.focus(composer)

    expect((await screen.findByRole('alert')).textContent).toMatch(/utkastbeskyttelsen kunne ikke bekreftes/i)
    expect((composer as HTMLTextAreaElement).disabled).toBe(false)
    expect(screen.queryByText(/en annen operatør skriver et utkast/i)).toBeNull()
  })

  it('persists a blurred draft before releasing a lease claim that was still in flight', async () => {
    let releaseLeaseClaim: () => void = () => undefined
    draftLeaseGate = new Promise<void>((resolve) => { releaseLeaseClaim = resolve })
    renderInbox()

    const ticketList = await screen.findByRole('list', { name: /saker/i })
    fireEvent.click(within(ticketList).getByRole('button', { name: /order marked delivered but missing/i }))

    const composer = await screen.findByRole('textbox', { name: /svar til maya solberg/i })
    fireEvent.focus(composer)
    fireEvent.input(composer, { target: { value: 'Hold this draft while the claim resolves.' } })
    fireEvent.blur(composer)

    expect(vi.mocked(fetch).mock.calls.some(([input, init]) =>
      String(input).endsWith(`/api/v1/inbox/conversations/${conversationSummary.id}/draft-lease`)
      && init?.method === 'DELETE',
    )).toBe(false)

    releaseLeaseClaim()

    await waitFor(() => expect(personalDraft).toEqual({
      body_text: 'Hold this draft while the claim resolves.',
      internal: false,
    }))
    await waitFor(() => expect(vi.mocked(fetch).mock.calls.some(([input, init]) =>
      String(input).endsWith(`/api/v1/inbox/conversations/${conversationSummary.id}/draft-lease`)
      && init?.method === 'DELETE',
    )).toBe(true))

    const calls = vi.mocked(fetch).mock.calls
    const saveIndex = calls.findIndex(([input, init]) =>
      String(input).endsWith(`/api/v1/inbox/conversations/${conversationSummary.id}/draft`)
      && init?.method === 'PUT',
    )
    const releaseIndex = calls.findIndex(([input, init]) =>
      String(input).endsWith(`/api/v1/inbox/conversations/${conversationSummary.id}/draft-lease`)
      && init?.method === 'DELETE',
    )
    expect(saveIndex).toBeGreaterThan(-1)
    expect(releaseIndex).toBeGreaterThan(saveIndex)
  })

  it('restores only the operator personal draft and saves later edits with its note mode', async () => {
    personalDraft = { body_text: 'Check the carrier scan before replying.', internal: true }
    renderInbox()

    const ticketList = await screen.findByRole('list', { name: /saker/i })
    fireEvent.click(within(ticketList).getByRole('button', { name: /order marked delivered but missing/i }))

    const composer = await screen.findByRole('textbox', { name: /legg til et internt notat/i })
    await waitFor(() => expect((composer as HTMLTextAreaElement).value).toBe('Check the carrier scan before replying.'))
    expect(await screen.findByText('Ditt lagrede utkast er gjenopprettet.')).toBeTruthy()

    fireEvent.input(composer, { target: { value: 'Carrier scan is still pending.' } })
    await waitFor(() => expect(personalDraft).toEqual({ body_text: 'Carrier scan is still pending.', internal: true }), { timeout: 2_000 })
  })

  it('does not retain personal drafts when the gateway enforces Zero Data Retention', async () => {
    draftPersistenceForbidden = true
    renderInbox()

    const ticketList = await screen.findByRole('list', { name: /saker/i })
    fireEvent.click(within(ticketList).getByRole('button', { name: /order marked delivered but missing/i }))

    expect(await screen.findByText(/zdr er aktiv: personlige utkast/i)).toBeTruthy()
    const composer = await screen.findByRole('textbox', { name: /svar til maya solberg/i })
    fireEvent.input(composer, { target: { value: 'This must stay only in the open field.' } })
    await new Promise((resolve) => window.setTimeout(resolve, 850))
    expect(personalDraft).toBeNull()
    expect(vi.mocked(fetch).mock.calls.some(([input, init]) =>
      String(input).endsWith(`/api/v1/inbox/conversations/${conversationSummary.id}/draft`)
      && init?.method === 'PUT',
    )).toBe(false)
  })

  it('shows unassigned incoming conversations in the default "Your inbox" view and hides other agents\' conversations', async () => {
    const unassigned = {
      ...conversationSummary,
      id: 'conv_unassigned_incoming',
      title: 'New Outlook email awaiting triage',
      assignee_user_id: '',
      assignee_name: '',
    }
    const otherAgent = {
      ...conversationSummary,
      id: 'conv_other_agent',
      title: 'Assigned to a teammate',
      assignee_user_id: 'user-other',
      assignee_name: 'Someone Else',
    }
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input)
      if (url.endsWith('/api/v1/auth/session')) {
        return jsonResponse({
          user: { id: 'user-demo', email: 'verevon@example.com', name: 'Verevon Demo', emailVerified: true },
        })
      }
      if (url.endsWith('/api/v1/me/session-context')) {
        return jsonResponse({ userId: 'user-demo', orgs: [{ id: 'org-demo', name: 'Verevon', role: 'owner' }] })
      }
      if (url.startsWith('/api/v1/inbox/conversations?')) {
        return jsonResponse([unassigned, otherAgent])
      }
      if (url.endsWith('/api/v1/inbox/inboxes')) {
        return jsonResponse([{ id: 'inbox-support', name: 'Commerce context' }])
      }
      return jsonResponse({})
    }))

    renderInbox()

    const ticketList = await screen.findByRole('list', { name: /saker/i })
    // Unassigned incoming mail must be visible in the default queue — this is the
    // conversation that a fresh, unassigned provider ingest produces.
    await waitFor(() =>
      expect(within(ticketList).getByRole('button', { name: /new outlook email awaiting triage/i })).toBeTruthy(),
    )
    // A conversation owned by a different agent stays out of "Your inbox".
    expect(within(ticketList).queryByRole('button', { name: /assigned to a teammate/i })).toBeNull()
  })

  it('reuses the same idempotency key when an ambiguous reply is retried unchanged', async () => {
    failFirstReply = true
    const randomUUID = vi.fn(() => 'manual-reply-1234567890')
    vi.stubGlobal('crypto', { randomUUID })
    renderInbox()

    const ticketList = await screen.findByRole('list', { name: /saker/i })
    fireEvent.click(within(ticketList).getByRole('button', { name: /order marked delivered but missing/i }))

    const composer = await screen.findByRole('textbox', { name: /svar til maya solberg/i })
    fireEvent.input(composer, { target: { value: submittedReply.body_text } })
    fireEvent.click(screen.getByRole('button', { name: /^send$/i }))

    expect(await screen.findByText(/vi kan ikke bekrefte om svaret ble sendt.*ikke send på nytt automatisk/i)).toBeTruthy()
    await waitFor(() => expect((composer as HTMLTextAreaElement).value).toBe(submittedReply.body_text))

    fireEvent.click(screen.getByRole('button', { name: /^send$/i }))
    expect(await screen.findByText('Svar sendt.')).toBeTruthy()

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

  it('reconciles a false-failure 502 by checking whether the ticket was actually resolved', async () => {
    const resolvableTicket = {
      id: 'ticket_resolve_1',
      org_id: 'org-demo',
      conversation_id: conversationSummary.id,
      ticket_key: 'TCK-200',
      status: 'open',
      priority: 'high',
      severity: 'high',
      category: 'delivery',
      intent: 'customer_follow_up',
      source: 'manual',
      created_at: '2026-08-02T12:00:00.000Z',
      updated_at: '2026-08-02T12:00:00.000Z',
    }
    let resolved = false
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input)
      if (url.endsWith('/api/v1/auth/session')) {
        return jsonResponse({ user: { id: 'user-demo', email: 'verevon@example.com', name: 'Verevon Demo', emailVerified: true } })
      }
      if (url.endsWith('/api/v1/me/session-context')) {
        return jsonResponse({ orgId: 'org-demo', orgs: [{ id: 'org-demo', name: 'Verevon', role: 'owner' }] })
      }
      if (url.startsWith('/api/v1/inbox/conversations?')) {
        return jsonResponse([conversationSummary])
      }
      if (url.endsWith('/api/v1/inbox/inboxes')) {
        return jsonResponse([{ id: 'inbox-support', name: 'Commerce context' }])
      }
      if (url.endsWith(`/api/v1/inbox/conversations/${conversationSummary.id}`)) {
        return jsonResponse(conversationDetail)
      }
      if (url.startsWith('/api/v1/tickets?')) {
        return jsonResponse(resolved ? [{ ...resolvableTicket, status: 'resolved' }] : [resolvableTicket])
      }
      if (url.endsWith('/api/v1/actions/execute') && init?.method === 'POST') {
        return jsonResponse({ actionId: 'tickets.update', runId: 'run_resolve', status: 'completed', auditId: 'audit_resolve', eventStream: '' })
      }
      if (url.endsWith(`/api/v1/tickets/${resolvableTicket.id}`) && (!init || !init.method)) {
        // executeTicketPatch's own follow-up getTicket() read (after the
        // actions/execute mutation above already durably recorded the status
        // change) is what's lost to a transient gateway error here.
        resolved = true
        return jsonResponse({ message: 'Bad Gateway' }, 502)
      }
      return jsonResponse({})
    }))

    renderInbox()

    const ticketList = await screen.findByRole('list', { name: /saker/i })
    fireEvent.click(within(ticketList).getByRole('button', { name: /order marked delivered but missing/i }))

    const resolveButtons = await screen.findAllByRole('button', { name: /løs sak|resolve ticket/i })
    fireEvent.click(resolveButtons[0]!)

    expect(await screen.findByText(/saken er løst og verifisert på nytt|ticket resolved and reread/i)).toBeTruthy()
    expect(screen.queryByText(/saken kunne ikke løses|the ticket could not be resolved/i)).toBeNull()
  })

  it('shows a real failure when resolving a ticket genuinely did not go through', async () => {
    const resolvableTicket = {
      id: 'ticket_resolve_1',
      org_id: 'org-demo',
      conversation_id: conversationSummary.id,
      ticket_key: 'TCK-200',
      status: 'open',
      priority: 'high',
      severity: 'high',
      category: 'delivery',
      intent: 'customer_follow_up',
      source: 'manual',
      created_at: '2026-08-02T12:00:00.000Z',
      updated_at: '2026-08-02T12:00:00.000Z',
    }
    vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input)
      if (url.endsWith('/api/v1/auth/session')) {
        return jsonResponse({ user: { id: 'user-demo', email: 'verevon@example.com', name: 'Verevon Demo', emailVerified: true } })
      }
      if (url.endsWith('/api/v1/me/session-context')) {
        return jsonResponse({ orgId: 'org-demo', orgs: [{ id: 'org-demo', name: 'Verevon', role: 'owner' }] })
      }
      if (url.startsWith('/api/v1/inbox/conversations?')) {
        return jsonResponse([conversationSummary])
      }
      if (url.endsWith('/api/v1/inbox/inboxes')) {
        return jsonResponse([{ id: 'inbox-support', name: 'Commerce context' }])
      }
      if (url.endsWith(`/api/v1/inbox/conversations/${conversationSummary.id}`)) {
        return jsonResponse(conversationDetail)
      }
      if (url.startsWith('/api/v1/tickets?')) {
        // Still open — the resolution genuinely didn't land.
        return jsonResponse([resolvableTicket])
      }
      if (url.endsWith('/api/v1/actions/execute') && init?.method === 'POST') {
        return jsonResponse({ actionId: 'tickets.update', runId: 'run_resolve', status: 'completed', auditId: 'audit_resolve', eventStream: '' })
      }
      if (url.endsWith(`/api/v1/tickets/${resolvableTicket.id}`) && (!init || !init.method)) {
        return jsonResponse({ message: 'Bad Gateway' }, 502)
      }
      return jsonResponse({})
    }))

    renderInbox()

    const ticketList = await screen.findByRole('list', { name: /saker/i })
    fireEvent.click(within(ticketList).getByRole('button', { name: /order marked delivered but missing/i }))

    const resolveButtons = await screen.findAllByRole('button', { name: /løs sak|resolve ticket/i })
    fireEvent.click(resolveButtons[0]!)

    expect(await screen.findByText(/saken kunne ikke løses|the ticket could not be resolved/i)).toBeTruthy()
    expect(screen.queryByText(/saken er løst og verifisert på nytt|ticket resolved and reread/i)).toBeNull()
  })
})
