// @vitest-environment jsdom

import { Route, Router } from '@solidjs/router'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import TicketingPage from '@/features/tickets/components/TicketingPage'
import type {
  SlaPolicy,
  SupportTicket,
  TicketAutomationRule,
  TicketChecklist,
  TicketMacro,
  TicketView,
} from '@/shared/api/tickets-client'

const conversation = {
  id: 'conv_refund_1',
  org_id: 'org-demo',
  inbox_id: 'inbox-support',
  title: 'Refund blocked after delivery issue',
  status: 'open',
  priority: 'high',
  channel: 'email',
  provider: 'email',
  assignee_user_id: 'user-demo',
  assignee_name: 'Velion Demo',
  last_message_preview: 'The customer needs a refund update before end of day.',
  last_message_at: '2026-06-18T09:00:00.000Z',
  contact: { id: 'contact-maya', name: 'Maya Solberg', email: 'maya@example.com' },
  tags: ['refund'],
  created_at: '2026-06-17T09:00:00.000Z',
  updated_at: '2026-06-18T09:00:00.000Z',
}

const checklist: TicketChecklist = {
  id: 'checklist_refund',
  org_id: 'org-demo',
  ticket_id: 'ticket_refund_1',
  name: 'Refund review',
  items: [
    {
      id: 'checkitem_confirm',
      org_id: 'org-demo',
      checklist_id: 'checklist_refund',
      label: 'Confirm refund eligibility',
      completed: false,
      position: 0,
      created_at: '2026-06-18T09:05:00.000Z',
      updated_at: '2026-06-18T09:05:00.000Z',
    },
  ],
  created_at: '2026-06-18T09:05:00.000Z',
  updated_at: '2026-06-18T09:05:00.000Z',
}

const ticketFixture: SupportTicket = {
  id: 'ticket_refund_1',
  org_id: 'org-demo',
  conversation_id: conversation.id,
  ticket_key: 'TCK-REFUND',
  status: 'open',
  priority: 'high',
  severity: 'high',
  category: 'refund',
  intent: 'refund_follow_up',
  assignee_user_id: 'user-demo',
  assignee_name: 'Velion Demo',
  team_id: 'billing',
  team_name: 'Billing',
  due_at: '2026-06-18T18:00:00.000Z',
  source: 'ai',
  ai_confidence: 0.91,
  ai_reason: 'Customer needs a billing owner and a refund update.',
  labels: ['refund', 'billing'],
  sla_state: 'risk',
  conversation,
  linked_resources: [{
    id: 'link_order',
    org_id: 'org-demo',
    ticket_id: 'ticket_refund_1',
    conversation_id: conversation.id,
    link_type: 'related',
    resource_kind: 'order',
    resource_id: 'order_1001',
    label: 'Order 1001',
    metadata: {},
    created_at: '2026-06-18T09:10:00.000Z',
  }],
  checklists: [checklist],
  created_at: '2026-06-18T09:00:00.000Z',
  updated_at: '2026-06-18T09:20:00.000Z',
}

const ticketViews: TicketView[] = [{
  id: 'view_refunds',
  org_id: 'org-demo',
  name: 'Refund handoffs',
  scope: 'team',
  visibility: 'sidebar',
  filter: { label: 'refund' },
  sort: { due_at: 'asc' },
  group_by: 'priority',
  sidebar_order: 1,
  created_at: '2026-06-18T09:00:00.000Z',
  updated_at: '2026-06-18T09:00:00.000Z',
}]

const refundMacro: TicketMacro = {
  id: 'macro_refund_handoff',
  org_id: 'org-demo',
  name: 'Refund handoff',
  description: 'Escalate billing and prepare customer update',
  visibility: 'team',
  team_id: 'billing',
  active: true,
  actions: { status: 'waiting_team', priority: 'urgent', labels: ['refund', 'handoff'] },
  conditions: { category: 'refund' },
  created_at: '2026-06-18T09:00:00.000Z',
  updated_at: '2026-06-18T09:00:00.000Z',
}

const macros: TicketMacro[] = [refundMacro]

const slaPolicies: SlaPolicy[] = [{
  id: 'sla_urgent',
  org_id: 'org-demo',
  name: 'Urgent escalation SLA',
  active: true,
  conditions: { priority: ['high', 'urgent'] },
  first_response_minutes: 60,
  next_response_minutes: 120,
  resolution_minutes: 720,
  created_at: '2026-06-18T09:00:00.000Z',
  updated_at: '2026-06-18T09:00:00.000Z',
}]

const automationRules: TicketAutomationRule[] = [{
  id: 'rule_refund',
  org_id: 'org-demo',
  name: 'Route refund tickets',
  event_name: 'ticket.created',
  active: true,
  conditions: { category: 'refund' },
  actions: { team_id: 'billing', labels: ['refund'] },
  created_at: '2026-06-18T09:00:00.000Z',
  updated_at: '2026-06-18T09:00:00.000Z',
}]

function jsonResponse(data: unknown, status = 200) {
  return new Response(JSON.stringify({ data }), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}

function mockTicketingGateway() {
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
        userId: 'user-demo',
        email: 'velion@example.com',
        name: 'Velion Demo',
        orgs: [{ id: 'org-demo', name: 'Velion', role: 'owner' }],
      })
    }
    if (url.startsWith('/api/v1/tickets?')) {
      return jsonResponse([ticketFixture])
    }
    if (url.endsWith('/api/v1/ticket-views')) return jsonResponse(ticketViews)
    if (url.endsWith('/api/v1/ticket-macros')) return jsonResponse(macros)
    if (url.endsWith('/api/v1/sla-policies')) return jsonResponse(slaPolicies)
    if (url.endsWith('/api/v1/ticket-automation-rules')) return jsonResponse(automationRules)
    if (url.endsWith(`/api/v1/tickets/${ticketFixture.id}/macros/${refundMacro.id}/run`)) {
      return jsonResponse({
        macro: refundMacro,
        ticket: { ...ticketFixture, status: 'waiting_team', priority: 'urgent', labels: ['refund', 'handoff'] },
      })
    }
    if (url.endsWith(`/api/v1/tickets/${ticketFixture.id}/checklists`) && init?.method === 'POST') {
      return jsonResponse({
        ...checklist,
        id: 'checklist_new',
        name: 'Resolution checklist',
        items: [],
      }, 201)
    }
    if (url.endsWith(`/api/v1/tickets/${ticketFixture.id}`) && init?.method === 'PATCH') {
      const body = JSON.parse(String(init.body ?? '{}')) as Partial<SupportTicket>
      return jsonResponse({ ...ticketFixture, ...body })
    }
    if (url.endsWith('/api/v1/social/drafts/from-inbox')) {
      return jsonResponse({
        post: {
          id: 'social_ticket_refund_1',
          title: 'Follow-up from Refund blocked after delivery issue',
          body: 'Draft follow-up',
          status: 'draft',
          scheduledAt: '2026-06-19T09:30:00.000Z',
          platforms: ['linkedin', 'x'],
          source: { kind: 'inbox', label: 'Ticket TCK-REFUND', href: '/tickets?queue=my' },
          approval: { required: true, state: 'not_requested' },
          media: [],
          previews: [],
        },
      })
    }
    return jsonResponse({})
  }))
}

function renderTicketing(path = '/tickets?queue=my') {
  window.history.pushState(null, '', path)
  return render(() => (
    <Router root={(props) => <>{props.children}</>}>
      <Route path="/tickets" component={TicketingPage} />
      <Route path="/*all" component={() => <div />} />
    </Router>
  ))
}

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
})

beforeEach(() => {
  mockTicketingGateway()
})

describe('TicketingPage', () => {
  it('renders the V2 ticket workspace with saved views, macros, SLA, checklist, and links', async () => {
    renderTicketing()

    expect((await screen.findAllByText('TCK-REFUND')).length).toBeGreaterThan(1)
    expect(screen.getByRole('heading', { name: /refund blocked after delivery issue/i })).toBeTruthy()
    expect(screen.getByText('Auto-created ticket')).toBeTruthy()
    expect(screen.getAllByText('Urgent escalation SLA').length).toBeGreaterThan(1)
    expect(screen.getByText('Order 1001')).toBeTruthy()
    expect(screen.getByText('Refund review')).toBeTruthy()

    const context = screen.getByRole('complementary', { name: 'Ticket context' })
    expect(within(context).getByText('Queue health')).toBeTruthy()
    expect(within(context).getByText('Escalate billing and prepare customer update')).toBeTruthy()
    expect(within(context).getByText('Route refund tickets')).toBeTruthy()
  })

  it('forwards route filters to the ticket list API', async () => {
    renderTicketing('/tickets?queue=my&sla_state=risk&priority=urgent&severity=critical')

    await screen.findAllByText('TCK-REFUND')

    const fetchMock = vi.mocked(fetch)
    expect(fetchMock.mock.calls.some(([url]) => {
      const value = String(url)
      return value.startsWith('/api/v1/tickets?') &&
        value.includes('queue=my') &&
        value.includes('sla_state=risk') &&
        value.includes('priority=urgent') &&
        value.includes('severity=critical')
    })).toBe(true)
  })

  it('runs macros, adds checklists, resolves tickets, and creates social follow-ups', async () => {
    renderTicketing()

    await screen.findAllByText('TCK-REFUND')
    const context = screen.getByRole('complementary', { name: 'Ticket context' })
    const macroDescription = within(context).getByText('Escalate billing and prepare customer update')
    const macroButton = macroDescription.closest('button')
    if (!macroButton) throw new Error('Expected refund handoff macro button')
    fireEvent.click(macroButton)
    await waitFor(() => expect(screen.getByText(/macro "refund handoff" applied/i)).toBeTruthy())

    fireEvent.click(screen.getByRole('button', { name: 'Add checklist' }))
    await waitFor(() => expect(screen.getByText('Checklist added.')).toBeTruthy())

    fireEvent.click(screen.getByRole('button', { name: 'Resolve' }))
    await waitFor(() => expect(screen.getByText('Ticket resolved.')).toBeTruthy())

    fireEvent.click(screen.getByRole('button', { name: 'Social follow-up' }))
    await waitFor(() => expect(window.location.pathname).toBe('/social/drafts'))

    const fetchMock = vi.mocked(fetch)
    expect(fetchMock.mock.calls.some(([url]) => String(url).endsWith(`/api/v1/tickets/${ticketFixture.id}/macros/${refundMacro.id}/run`))).toBe(true)
    expect(fetchMock.mock.calls.some(([url, init]) => String(url).endsWith(`/api/v1/tickets/${ticketFixture.id}/checklists`) && init?.method === 'POST')).toBe(true)
    expect(fetchMock.mock.calls.some(([url, init]) => String(url).endsWith(`/api/v1/tickets/${ticketFixture.id}`) && init?.method === 'PATCH' && String(init.body).includes('resolved'))).toBe(true)
    expect(fetchMock.mock.calls.some(([url]) => String(url).endsWith('/api/v1/social/drafts/from-inbox'))).toBe(true)
  })
})
