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

function jsonResponse(data: unknown, status = 200) {
  return new Response(JSON.stringify({ data }), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}

function mockInboxGateway() {
  vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL) => {
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
})
