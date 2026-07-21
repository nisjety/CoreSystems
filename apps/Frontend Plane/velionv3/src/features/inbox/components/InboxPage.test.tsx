// @vitest-environment jsdom

import { Route, Router } from '@solidjs/router'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import InboxPage from '@/features/inbox/components/InboxPage'

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
        orgs: [{ id: 'org-demo', name: 'Velion', role: 'owner' }],
      })
    }
    if (url.startsWith('/api/v1/inbox/conversations?')) {
      return jsonResponse([conversationSummary])
    }
    if (url.endsWith(`/api/v1/inbox/conversations/${conversationSummary.id}`)) {
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
  window.sessionStorage.clear()
  vi.unstubAllGlobals()
})

beforeEach(() => {
  failFirstReply = false
  replyAttempts = 0
  mockInboxGateway()
})

describe('InboxPage', () => {
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
          user: { id: 'user-demo', email: 'velion@example.com', name: 'Velion Demo', emailVerified: true },
        })
      }
      if (url.endsWith('/api/v1/me/session-context')) {
        return jsonResponse({ userId: 'user-demo', orgs: [{ id: 'org-demo', name: 'Velion', role: 'owner' }] })
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

    const ticketList = await screen.findByRole('list', { name: /tickets/i })
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
