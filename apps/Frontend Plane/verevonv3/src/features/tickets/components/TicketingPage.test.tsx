// @vitest-environment jsdom

import { createRouter, memoryHistory } from '@solidjs/router'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import TicketingPage from '@/features/tickets/components/TicketingPage'
import type {
  SlaPolicy,
  SupportTicket,
  TicketAutomationRule,
  TicketChecklist,
  TicketMacro,
  TicketTeam,
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
  assignee_name: 'Verevon Demo',
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
  assignee_name: 'Verevon Demo',
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
  }, {
    id: 'link_incident',
    org_id: 'org-demo',
    ticket_id: 'ticket_refund_1',
    conversation_id: conversation.id,
    link_type: 'child',
    resource_kind: 'ticket',
    resource_id: 'ticket_incident_1',
    label: 'TCK-INCIDENT',
    metadata: {},
    linked_ticket: {
      id: 'ticket_incident_1',
      ticket_key: 'TCK-INCIDENT',
      status: 'waiting_team',
      work_type: 'incident',
    },
    created_at: '2026-06-18T09:11:00.000Z',
  }],
  checklists: [checklist],
  side_conversations: [{
    id: 'side_refund_1',
    org_id: 'org-demo',
    ticket_id: 'ticket_refund_1',
    subject: 'Confirm refund exception',
    status: 'open',
    created_by_user_id: 'user-demo',
    messages: [{
      id: 'side_message_1',
      org_id: 'org-demo',
      side_conversation_id: 'side_refund_1',
      body_text: 'Can billing confirm the exception before we reply?',
      created_by_user_id: 'user-demo',
      created_at: '2026-06-18T09:30:00.000Z',
    }],
    created_at: '2026-06-18T09:30:00.000Z',
    updated_at: '2026-06-18T09:30:00.000Z',
  }],
  created_at: '2026-06-18T09:00:00.000Z',
  updated_at: '2026-06-18T09:20:00.000Z',
}

const relatedTicketFixture: SupportTicket = {
  ...ticketFixture,
  id: 'ticket_carrier_1',
  conversation_id: 'conv_carrier_1',
  ticket_key: 'TCK-CARRIER',
  status: 'waiting_team',
  category: 'delivery',
  intent: 'carrier_investigation',
  linked_resources: [],
  checklists: [],
  conversation: {
    ...conversation,
    id: 'conv_carrier_1',
    title: 'Carrier investigation for delayed replacement',
  },
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

const ticketTeams: TicketTeam[] = [{
  id: 'billing',
  org_id: 'org-demo',
  name: 'Billing',
  description: 'Refund and payment support',
  active: true,
  created_at: '2026-06-01T09:00:00.000Z',
  updated_at: '2026-06-01T09:00:00.000Z',
}, {
  id: 'delivery',
  org_id: 'org-demo',
  name: 'Delivery',
  active: true,
  created_at: '2026-06-01T09:00:00.000Z',
  updated_at: '2026-06-01T09:00:00.000Z',
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

const ticketActivity = [
	{ id: 'audit_macro', action: 'ticket.macro_run', created_at: '2026-06-18T09:12:00.000Z' },
	{ id: 'audit_checklist', action: 'ticket.checklist_item_updated', created_at: '2026-06-18T09:11:00.000Z' },
  { id: 'audit_link', action: 'ticket.linked', resource_kind: 'order', created_at: '2026-06-18T09:10:00.000Z' },
  { id: 'audit_created', action: 'ticket.created', created_at: '2026-06-18T09:00:00.000Z' },
]

let ticketsError: Response | null = null
let ticketingRole = 'owner'
let incidentTicketing = false
let ticketList: SupportTicket[] = []

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
          email: 'verevon@example.com',
          name: 'Verevon Demo',
          emailVerified: true,
        },
      })
    }
    if (url.endsWith('/api/v1/me/session-context')) {
      return jsonResponse({
        userId: 'user-demo',
        email: 'verevon@example.com',
        name: 'Verevon Demo',
		orgs: [{ id: 'org-demo', name: 'Verevon', role: ticketingRole }],
      })
    }
    if (url.startsWith('/api/v1/tickets?')) {
      if (ticketsError) return ticketsError
      return jsonResponse(incidentTicketing
        ? [{ ...ticketFixture, work_type: 'incident' }, relatedTicketFixture]
        : ticketList)
    }
		if (url.includes('/api/v1/tickets/') && url.includes('/activity?limit=20')) return jsonResponse(ticketActivity)
    if (url.endsWith('/api/v1/actions/execute') && init?.method === 'POST') {
      const body = JSON.parse(String(init.body ?? '{}')) as { actionId?: string }
      return jsonResponse({
        actionId: body.actionId ?? 'tickets.update',
        runId: `run_${body.actionId ?? 'tickets.update'}`,
        status: 'completed',
        auditId: `audit_${body.actionId ?? 'tickets.update'}`,
        eventStream: '/api/v1/actions/events/test',
      })
    }
    if (url.endsWith('/api/v1/ticket-views')) return jsonResponse(ticketViews)
    if (url.endsWith('/api/v1/ticket-teams')) return jsonResponse(ticketTeams)
    if (url.endsWith('/api/v1/ticket-macros')) return jsonResponse(macros)
    if (url.endsWith('/api/v1/sla-policies')) return jsonResponse(slaPolicies)
    if (url.endsWith('/api/v1/ticket-automation-rules') && init?.method === 'POST') {
      const body = JSON.parse(String(init.body ?? '{}')) as Partial<TicketAutomationRule>
      return jsonResponse({ ...automationRules[0]!, ...body }, 201)
    }
    if (url.endsWith('/api/v1/ticket-automation-rules')) return jsonResponse(automationRules)
    if (url.endsWith('/api/v1/incidents') && init?.method === 'POST') {
      return jsonResponse({ id: 'incident_1', incident_key: 'INC-TEST', title: 'Delivery outage', status: 'declared', severity: 'critical', ticket_links: [] }, 201)
    }
    if (url.endsWith('/api/v1/incidents')) return jsonResponse([])
    if (url.endsWith('/api/v1/problems') && init?.method === 'POST') {
      return jsonResponse({ id: 'problem_1', problem_key: 'PRB-TEST', title: 'Carrier timeout', status: 'investigating' }, 201)
    }
    if (url.endsWith('/api/v1/problems')) return jsonResponse([])
    if (url.endsWith('/api/v1/incidents/incident_1/tickets') && init?.method === 'POST') {
      return jsonResponse({ id: 'incident_ticket_1', incident_id: 'incident_1', ticket_id: ticketFixture.id, relationship: 'affected' }, 201)
    }
    if (url.endsWith(`/api/v1/ticket-automation-rules/${automationRules[0]!.id}`) && init?.method === 'PATCH') {
      const body = JSON.parse(String(init.body ?? '{}')) as Partial<TicketAutomationRule>
      return jsonResponse({ ...automationRules[0]!, ...body })
    }
    if (url.endsWith(`/api/v1/tickets/${ticketFixture.id}`) && init?.method === 'PATCH') {
      const body = JSON.parse(String(init.body ?? '{}')) as Partial<SupportTicket>
      return jsonResponse({ ...ticketFixture, ...body })
    }
    if (url.endsWith(`/api/v1/tickets/${ticketFixture.id}`) && (!init?.method || init.method === 'GET')) {
      return jsonResponse(ticketFixture)
    }
    if (url.endsWith(`/api/v1/tickets/${relatedTicketFixture.id}`) && (!init?.method || init.method === 'GET')) {
      return jsonResponse(relatedTicketFixture)
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
  // `memoryHistory` is an isolated in-memory router adapter: it never writes
  // to `window.location`, by design (browserHistory is what syncs the real
  // location in production). Tests that need to assert a navigation target
  // must read the router's own history entry, not `window.location`.
  const history = memoryHistory(path)
  const TestRouter = createRouter({
    routes: [
      { path: '/tickets', component: TicketingPage },
      { path: '/*all', component: () => <div /> },
    ],
    history,
    explicitLinks: true,
  })
  render(() => <TestRouter>{(props) => <>{props.children}</>}</TestRouter>)
  return { history }
}

function openTicketWorkspaceTab(name: RegExp) {
  const tabs = screen.getByRole('tablist', { name: /saksarbeidsområde|ticket workspace/i })
  fireEvent.click(within(tabs).getByRole('tab', { name }))
  flush()
}

function openTicketContextTab(name: RegExp) {
  const tabs = screen.getByRole('tablist', { name: /sakkontekst|ticket context/i })
  fireEvent.click(within(tabs).getByRole('tab', { name }))
  flush()
}

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
})

beforeEach(() => {
  ticketsError = null
	ticketingRole = 'owner'
  incidentTicketing = false
  ticketList = [ticketFixture, relatedTicketFixture]
  mockTicketingGateway()
})

describe('TicketingPage', () => {
  it('shows only taxonomy-backed repeat signals from the active ticket queue', async () => {
    ticketList = [
      ticketFixture,
      {
        ...ticketFixture,
        id: 'ticket_refund_2',
        ticket_key: 'TCK-REFUND-2',
        conversation_id: 'conv_refund_2',
        status: 'waiting_customer',
        conversation: { ...conversation, id: 'conv_refund_2', title: 'Refund update still pending' },
      },
      relatedTicketFixture,
    ]

    renderTicketing('/tickets?queue=all')

    const signals = await screen.findByRole('region', { name: /Recurring support signals|Gjentatte støttesignaler/i })
    expect(within(signals).getByText('refund')).toBeTruthy()
    expect(within(signals).getByText('refund follow up')).toBeTruthy()
    expect(within(signals).getByText(/2 active tickets|2 aktive saker/i)).toBeTruthy()
    expect(within(signals).getByText('TCK-REFUND, TCK-REFUND-2')).toBeTruthy()
    expect(within(signals).getByText(/does not infer semantic similarity|tolker ikke semantisk likhet/i)).toBeTruthy()
  })

  it('requires an exact reviewed ticket set before applying a narrow bulk status update', async () => {
    renderTicketing('/tickets?queue=all')

    await screen.findAllByText('TCK-REFUND')
    fireEvent.click(screen.getByRole('checkbox', { name: 'Velg alle synlige saker' }))
    flush()
    expect(screen.getByText('2 valgt')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Gjennomgå endring' }))

    const review = await screen.findByRole('dialog', { name: 'Bekreft masseendring av status' })
    expect(within(review).getByText('TCK-REFUND')).toBeTruthy()
    expect(within(review).getByText('TCK-CARRIER')).toBeTruthy()
    expect(within(review).getByText(/hver sak får sin egen reviderbare handling/i)).toBeTruthy()
    expect(vi.mocked(fetch).mock.calls.some(([url, init]) => String(url).endsWith('/api/v1/actions/execute') && init?.method === 'POST')).toBe(false)

    fireEvent.click(within(review).getByRole('button', { name: 'Oppdater status' }))
    await waitFor(() => expect(screen.getByRole('status').textContent).toMatch(/status ble oppdatert for 2 saker/i))

    const statusUpdates = vi.mocked(fetch).mock.calls
      .filter(([url, init]) => String(url).endsWith('/api/v1/actions/execute') && init?.method === 'POST')
      .map(([, init]) => JSON.parse(String(init?.body)))
    expect(statusUpdates).toHaveLength(2)
    // executeBulkTicketStatusUpdate() awaits one patch per ticket in selection
    // order, precisely so a partial failure is attributable. Asserting the
    // ordered, exact request bodies (rather than arrayContaining) is what keeps
    // that documented contract — and the "own auditable action per ticket"
    // promise in the review dialog — from silently regressing.
    expect(statusUpdates).toEqual([
      { actionId: 'tickets.update', idempotencyKey: expect.any(String), input: { ticketId: ticketFixture.id, status: 'waiting_team' } },
      { actionId: 'tickets.update', idempotencyKey: expect.any(String), input: { ticketId: relatedTicketFixture.id, status: 'waiting_team' } },
    ])
  })

  it('renders the V2 ticket workspace with saved views, macros, SLA, checklist, and links', async () => {
    renderTicketing()

    expect((await screen.findAllByText('TCK-REFUND')).length).toBeGreaterThan(1)
    expect(screen.getByRole('heading', { name: /refund blocked after delivery issue/i })).toBeTruthy()
    expect(screen.getByText('Auto-opprettet sak')).toBeTruthy()
    const workspaceTabs = screen.getByRole('tablist', { name: /saksarbeidsområde|ticket workspace/i })
    expect(within(workspaceTabs).getByRole('tab', { name: /samtale|conversation/i })).toBeTruthy()
    expect(within(workspaceTabs).getByRole('tab', { name: /relatert|related/i })).toBeTruthy()
    openTicketWorkspaceTab(/samtale|conversation/i)
    expect(screen.getByText(/customer conversation|kundesamtale/i)).toBeTruthy()
    openTicketWorkspaceTab(/relatert|related/i)
    expect(screen.getByText('Order 1001')).toBeTruthy()
    expect(screen.getByText('TCK-INCIDENT')).toBeTruthy()
    expect(screen.getByText('Underordnet sak · Venter på team · Hendelser')).toBeTruthy()

    const context = screen.getByRole('complementary', { name: 'Sakkontekst' })
    expect(within(context).getByText('Køhelse')).toBeTruthy()
    openTicketContextTab(/handlinger|actions/i)
    expect(within(context).getByText('Escalate billing and prepare customer update')).toBeTruthy()
    expect(screen.getByText('Refund review')).toBeTruthy()
    openTicketContextTab(/revisjon|audit/i)
    expect(within(context).getByText('Confirm refund exception')).toBeTruthy()
    expect(within(context).getByText('Can billing confirm the exception before we reply?')).toBeTruthy()
    expect(within(context).getByText(/kun internt.*aldri en kundeoppdatering/i)).toBeTruthy()
    expect(within(context).getByText('Route refund tickets')).toBeTruthy()
  })

  it('creates and replies to a ticket-side conversation only through internal action contracts', async () => {
    renderTicketing()

    await screen.findAllByText('TCK-REFUND')
    openTicketContextTab(/revisjon|audit/i)
    fireEvent.input(screen.getByRole('textbox', { name: 'Emne for intern samtale' }), { target: { value: 'Check billing decision' } })
    fireEvent.input(screen.getByRole('textbox', { name: 'Første interne melding' }), { target: { value: 'Is the manual refund approved?' } })
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Start intern samtale' }))

    await waitFor(() => expect(vi.mocked(fetch).mock.calls.some(([url, init]) =>
      String(url).endsWith('/api/v1/actions/execute')
      && init?.method === 'POST'
      && String(init.body).includes('tickets.create_side_conversation'),
    )).toBe(true))
    const createRequest = vi.mocked(fetch).mock.calls.find(([url, init]) =>
      String(url).endsWith('/api/v1/actions/execute')
      && init?.method === 'POST'
      && String(init.body).includes('tickets.create_side_conversation'),
    )
    expect(JSON.parse(String(createRequest?.[1]?.body))).toMatchObject({
      actionId: 'tickets.create_side_conversation',
      input: { ticketId: ticketFixture.id, subject: 'Check billing decision', bodyText: 'Is the manual refund approved?' },
    })

    fireEvent.input(screen.getByRole('textbox', { name: 'Svar på Confirm refund exception' }), { target: { value: 'Billing approved it.' } })
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Svar' }))
    await waitFor(() => expect(vi.mocked(fetch).mock.calls.some(([url, init]) =>
      String(url).endsWith('/api/v1/actions/execute')
      && init?.method === 'POST'
      && String(init.body).includes('tickets.add_side_conversation_message'),
    )).toBe(true))
    const replyRequest = vi.mocked(fetch).mock.calls.find(([url, init]) =>
      String(url).endsWith('/api/v1/actions/execute')
      && init?.method === 'POST'
      && String(init.body).includes('tickets.add_side_conversation_message'),
    )
    expect(JSON.parse(String(replyRequest?.[1]?.body))).toMatchObject({
      actionId: 'tickets.add_side_conversation_message',
      input: { ticketId: ticketFixture.id, sideConversationId: 'side_refund_1', bodyText: 'Billing approved it.' },
    })
  })

	it('shows the tenant-scoped ticket audit feed instead of reconstructing current ticket state', async () => {
		renderTicketing()

		await screen.findAllByText('TCK-REFUND')
		openTicketWorkspaceTab(/aktivitet|activity/i)
		expect(await screen.findByText('Lenket order')).toBeTruthy()
		expect(screen.getByText('Saken ble opprettet')).toBeTruthy()
		expect(screen.getByText('Makro ble kjørt')).toBeTruthy()
		expect(screen.getByText('Sjekklistestatus ble oppdatert')).toBeTruthy()
		expect(vi.mocked(fetch).mock.calls.some(([url]) => String(url).includes(`/api/v1/tickets/${ticketFixture.id}/activity?limit=20`))).toBe(true)
	})

  it('declares a separate incident and links the current ticket without treating the ticket as the incident', async () => {
    incidentTicketing = true
    renderTicketing()

    await screen.findAllByText('TCK-REFUND')
    openTicketWorkspaceTab(/relatert|related/i)
    await screen.findByText('Hendelser og problemer')
    expect(screen.getByText('Egne operative poster. Ingen status endres automatisk på den lenkede saken.')).toBeTruthy()
    fireEvent.input(screen.getByRole('textbox', { name: 'Hendelsestittel' }), { target: { value: 'Delivery outage' } })
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Erklær hendelse' }))

    await waitFor(() => expect(screen.getByText('Hendelsen INC-TEST er erklært og saken er lenket.')).toBeTruthy())
    const calls = vi.mocked(fetch).mock.calls
    expect(calls.some(([url, init]) => String(url).endsWith('/api/v1/incidents') && init?.method === 'POST' && String(init.body).includes('Delivery outage'))).toBe(true)
    expect(calls.some(([url, init]) => String(url).endsWith('/api/v1/incidents/incident_1/tickets') && init?.method === 'POST' && String(init.body).includes('affected'))).toBe(true)
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

  it('requires review before running macros, then adds checklists, resolves tickets, and creates social follow-ups', async () => {
    const { history } = renderTicketing()

    await screen.findAllByText('TCK-REFUND')
    openTicketContextTab(/handlinger|actions/i)
    const context = screen.getByRole('complementary', { name: 'Sakkontekst' })
    const macroDescription = within(context).getByText('Escalate billing and prepare customer update')
    const macroButton = macroDescription.closest('button')
    if (!macroButton) throw new Error('Expected refund handoff macro button')
    fireEvent.click(macroButton)
    const review = await screen.findByRole('dialog', { name: 'Bekreft makro' })
    expect(within(review).getByText('Gjennomgå makro før kjøring')).toBeTruthy()
    expect(within(review).getByText(/waiting_team/)).toBeTruthy()
    expect(vi.mocked(fetch).mock.calls.some(([url]) => String(url).endsWith('/api/v1/actions/execute'))).toBe(false)
    fireEvent.click(within(review).getByRole('button', { name: 'Kjør makro' }))
    await waitFor(() => expect(screen.getByText(/makroen "refund handoff" ble kjørt/i)).toBeTruthy())

    fireEvent.click(screen.getByRole('button', { name: 'Legg til sjekkliste' }))
    await waitFor(() => expect(screen.getByText('Sjekkliste lagt til.')).toBeTruthy())

    fireEvent.click(screen.getByRole('button', { name: 'Løs' }))
    const resolutionReview = await screen.findByRole('dialog', { name: 'Bekreft løsning med åpne underordnede saker' })
    expect(within(resolutionReview).getByText('Underordnede saker er fortsatt åpne')).toBeTruthy()
    expect(within(resolutionReview).getByText(/TCK-INCIDENT.*Venter på team.*Hendelser/)).toBeTruthy()
    expect(vi.mocked(fetch).mock.calls.some(([url, init]) => String(url).endsWith('/api/v1/actions/execute') && init?.method === 'POST' && String(init.body).includes('tickets.resolve'))).toBe(false)
    fireEvent.click(within(resolutionReview).getByRole('button', { name: 'Løs likevel' }))
    await waitFor(() => expect(screen.getByText('Saken er løst.')).toBeTruthy())

    fireEvent.click(screen.getByRole('button', { name: 'Sosial oppfølging' }))
    await waitFor(() => expect(history.get()).toBe('/social/drafts'))

    const fetchMock = vi.mocked(fetch)
    expect(fetchMock.mock.calls.some(([url, init]) => String(url).endsWith('/api/v1/actions/execute') && init?.method === 'POST' && String(init.body).includes('tickets.run_macro'))).toBe(true)
    expect(fetchMock.mock.calls.some(([url, init]) => String(url).endsWith('/api/v1/actions/execute') && init?.method === 'POST' && String(init.body).includes('tickets.create_checklist'))).toBe(true)
    expect(fetchMock.mock.calls.some(([url, init]) => String(url).endsWith('/api/v1/actions/execute') && init?.method === 'POST' && String(init.body).includes('tickets.resolve'))).toBe(true)
    expect(fetchMock.mock.calls.some(([url]) => String(url).endsWith('/api/v1/social/drafts/from-inbox'))).toBe(true)
  })

  it('sets team follow-up separately from the SLA due date through the action gateway', async () => {
    renderTicketing()

    await screen.findAllByText('TCK-REFUND')
    fireEvent.click(screen.getByRole('button', { name: 'Oppfølging i morgen' }))
    await waitFor(() => expect(screen.getByText('Teamoppfølgingen er satt til i morgen.')).toBeTruthy())

    const execution = vi.mocked(fetch).mock.calls.find(([url, init]) =>
      String(url).endsWith('/api/v1/actions/execute')
      && init?.method === 'POST'
      && String(init.body).includes('tickets.update')
      && String(init.body).includes('followUpAt'),
    )
    const input = JSON.parse(String(execution?.[1]?.body)) as {
      actionId?: string
      input?: { ticketId?: string; followUpAt?: string; dueAt?: string }
    }
    expect(input.actionId).toBe('tickets.update')
    expect(input.input?.ticketId).toBe(ticketFixture.id)
    expect(input.input?.followUpAt).toMatch(/^\d{4}-\d{2}-\d{2}T/)
    expect(input.input?.dueAt).toBeUndefined()
  })

  it('routes through the canonical Ticketing team directory, not an Inbox group', async () => {
    renderTicketing()

    await screen.findAllByText('TCK-REFUND')
    const routeSelect = screen.getByRole('combobox', { name: 'Rute saken til et kanonisk team' })
    expect(within(routeSelect).getByRole('option', { name: 'Delivery' })).toBeTruthy()
    fireEvent.change(routeSelect, { target: { value: 'delivery' } })

    await waitFor(() => expect(screen.getByText('Saken er rutet til Delivery.')).toBeTruthy())
    const execution = vi.mocked(fetch).mock.calls.find(([url, init]) =>
      String(url).endsWith('/api/v1/actions/execute')
      && init?.method === 'POST'
      && String(init.body).includes('tickets.assign')
      && String(init.body).includes('delivery'),
    )
    expect(JSON.parse(String(execution?.[1]?.body))).toMatchObject({
      actionId: 'tickets.assign',
      input: { ticketId: ticketFixture.id, teamId: 'delivery', teamName: 'Delivery' },
    })
  })

  it('lets an operator manage the canonical Ticketing team directory', async () => {
    renderTicketing()

    await screen.findAllByText('TCK-REFUND')
    openTicketContextTab(/handlinger|actions/i)
    fireEvent.input(screen.getByRole('textbox', { name: 'Nytt Ticketing-teamnavn' }), { target: { value: 'Escalations' } })
    fireEvent.input(screen.getByRole('textbox', { name: 'Team-beskrivelse' }), { target: { value: 'Critical incident handoffs' } })
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Opprett team' }))

    await waitFor(() => expect(screen.getByText('Teamet Escalations er opprettet.')).toBeTruthy())
    const createRequest = vi.mocked(fetch).mock.calls.find(([url, init]) =>
      String(url).endsWith('/api/v1/ticket-teams')
      && init?.method === 'POST'
      && String(init.body).includes('Escalations'),
    )
    expect(JSON.parse(String(createRequest?.[1]?.body))).toMatchObject({
      name: 'Escalations',
      description: 'Critical incident handoffs',
      active: true,
    })

	const billingTeam = screen.getByText('Refund and payment support').closest('li')
	expect(billingTeam).not.toBeNull()
	fireEvent.click(within(billingTeam!).getByRole('button', { name: 'Deaktiver' }))
    await waitFor(() => expect(screen.getByText('Billing er deaktivert.')).toBeTruthy())
    expect(vi.mocked(fetch).mock.calls.some(([url, init]) =>
      String(url).endsWith('/api/v1/ticket-teams/billing')
      && init?.method === 'PATCH'
      && String(init.body).includes('"active":false'),
    )).toBe(true)
  })

  it('creates a bounded durable macro from the Rules workspace', async () => {
    renderTicketing('/tickets?queue=rules')
    await screen.findAllByText('Refund handoff')
    fireEvent.input(screen.getByRole('textbox', { name: 'Makronavn' }), { target: { value: 'Delivery handoff' } })
    fireEvent.input(screen.getByRole('textbox', { name: 'Makrobeskrivelse' }), { target: { value: 'Queue carrier investigation' } })
    fireEvent.change(screen.getByRole('combobox', { name: 'Makrostatus' }), { target: { value: 'waiting_team' } })
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Opprett makro' }))
    await waitFor(() => expect(vi.mocked(fetch).mock.calls.some(([url, init]) => String(url).endsWith('/api/v1/actions/execute') && init?.method === 'POST' && String(init.body).includes('tickets.create_macro'))).toBe(true))
    const request = vi.mocked(fetch).mock.calls.find(([url, init]) => String(url).endsWith('/api/v1/actions/execute') && init?.method === 'POST' && String(init.body).includes('tickets.create_macro'))
    expect(JSON.parse(String(request?.[1]?.body))).toMatchObject({
      actionId: 'tickets.create_macro',
      input: { name: 'Delivery handoff', description: 'Queue carrier investigation', visibility: 'team', status: 'waiting_team' },
    })
  })

  it('lets an operator pause an active automation rule from the Rules workspace', async () => {
    renderTicketing('/tickets?queue=rules')

    await screen.findAllByText('Route refund tickets')
    fireEvent.click(screen.getByRole('button', { name: 'Deaktiver regel Route refund tickets' }))

    await waitFor(() => expect(screen.getByText('Regelen Route refund tickets er satt på pause.')).toBeTruthy())
    const request = vi.mocked(fetch).mock.calls.find(([url, init]) =>
      String(url).endsWith('/api/v1/ticket-automation-rules/rule_refund')
      && init?.method === 'PATCH',
    )
    expect(JSON.parse(String(request?.[1]?.body))).toEqual({ active: false })
  })

  it('creates a bounded deterministic automation rule without raw JSON', async () => {
    renderTicketing('/tickets?queue=rules')
    await screen.findByRole('textbox', { name: 'Regelnavn' })
    await waitFor(() => expect(vi.mocked(fetch).mock.calls.some(([url]) =>
      String(url).endsWith('/api/v1/ticket-automation-rules'),
    )).toBe(true))
    fireEvent.input(screen.getByRole('textbox', { name: 'Regelnavn' }), { target: { value: 'Prioritise refund cases' } })
    fireEvent.input(screen.getByRole('textbox', { name: 'Betingelsesverdi' }), { target: { value: 'refund' } })
    fireEvent.input(screen.getByRole('textbox', { name: 'Handlingsverdi' }), { target: { value: 'urgent' } })
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Opprett regel' }))
    await waitFor(() => expect(vi.mocked(fetch).mock.calls.some(([url, init]) =>
      String(url).endsWith('/api/v1/ticket-automation-rules') && init?.method === 'POST',
    )).toBe(true))
    const request = vi.mocked(fetch).mock.calls.find(([url, init]) =>
      String(url).endsWith('/api/v1/ticket-automation-rules') && init?.method === 'POST',
    )
    expect(JSON.parse(String(request?.[1]?.body))).toEqual({
      name: 'Prioritise refund cases', event_name: 'ticket.created', active: true,
      conditions: { category: 'refund' }, actions: { priority: 'urgent' },
    })
  })

  it('offers canonical work type as a bounded automation condition', async () => {
    renderTicketing('/tickets?queue=rules')
    await screen.findByRole('textbox', { name: 'Regelnavn' })
    fireEvent.change(screen.getByRole('combobox', { name: 'Regelbetingelse' }), { target: { value: 'work_type' } })
    flush()
    fireEvent.change(screen.getByRole('combobox', { name: 'Arbeidstype' }), { target: { value: 'incident' } })
    expect(screen.queryByRole('textbox', { name: 'Betingelsesverdi' })).toBeNull()
    expect((screen.getByRole('combobox', { name: 'Arbeidstype' }) as HTMLSelectElement).value).toBe('incident')
  })

  it('keeps automation state visible but blocks non-admin rule changes', async () => {
    ticketingRole = 'member'
    renderTicketing('/tickets?queue=rules')
    expect(await screen.findByText('Bare eiere og administratorer kan endre automatiseringsregler.')).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Opprett regel' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Deaktiver regel Route refund tickets' })).toBeNull()
  })

  it('recognizes a case-variant administrator role from session context', async () => {
    ticketingRole = 'ADMIN'
    renderTicketing('/tickets?queue=rules')
    expect(await screen.findByRole('button', { name: 'Opprett regel' })).toBeTruthy()
  })

  it('serializes a label action as the canonical string list', async () => {
    renderTicketing('/tickets?queue=rules')
    await screen.findByRole('textbox', { name: 'Regelnavn' })
    await waitFor(() => expect(vi.mocked(fetch).mock.calls.some(([url]) => String(url).endsWith('/api/v1/ticket-automation-rules'))).toBe(true))
    fireEvent.input(screen.getByRole('textbox', { name: 'Regelnavn' }), { target: { value: 'Tag urgent refunds' } })
    fireEvent.input(screen.getByRole('textbox', { name: 'Betingelsesverdi' }), { target: { value: 'refund' } })
    fireEvent.change(screen.getByRole('combobox', { name: 'Regelhandling' }), { target: { value: 'labels' } })
    flush()
    fireEvent.input(screen.getByRole('textbox', { name: 'Handlingsverdi' }), { target: { value: 'refund, urgent' } })
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Opprett regel' }))
    await waitFor(() => expect(vi.mocked(fetch).mock.calls.some(([url, init]) => String(url).endsWith('/api/v1/ticket-automation-rules') && init?.method === 'POST')).toBe(true))
    const request = vi.mocked(fetch).mock.calls.find(([url, init]) => String(url).endsWith('/api/v1/ticket-automation-rules') && init?.method === 'POST')
    expect(JSON.parse(String(request?.[1]?.body))).toMatchObject({ actions: { labels: ['refund', 'urgent'] } })
  })

  it('links a distinct ticket through the audited work-dependency action', async () => {
    renderTicketing()

    await screen.findAllByText('TCK-REFUND')
    openTicketWorkspaceTab(/relatert|related/i)
    fireEvent.change(screen.getByRole('combobox', { name: 'Sak som skal lenkes' }), { target: { value: relatedTicketFixture.id } })
    fireEvent.change(screen.getByRole('combobox', { name: 'Type saksforhold' }), { target: { value: 'child' } })
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Koble sak' }))

    await waitFor(() => expect(screen.getByText(/saksforhold til tck-carrier ble lagt til/i)).toBeTruthy())
    const execution = vi.mocked(fetch).mock.calls.find(([url, init]) =>
      String(url).endsWith('/api/v1/actions/execute')
      && init?.method === 'POST'
      && String(init.body).includes('tickets.link_resource'),
    )
    expect(JSON.parse(String(execution?.[1]?.body))).toMatchObject({
      actionId: 'tickets.link_resource',
      input: {
        ticketId: ticketFixture.id,
        linkType: 'child',
        resourceKind: 'ticket',
        resourceId: relatedTicketFixture.id,
        label: relatedTicketFixture.ticket_key,
      },
    })
  })

  it('shows an unavailable state instead of a false empty queue when ticket loading fails', async () => {
    ticketsError = new Response(JSON.stringify({
      error: { code: 'upstream_unavailable', message: 'Conversation core is unavailable.' },
    }), {
      headers: { 'Content-Type': 'application/json' },
      status: 503,
    })

    renderTicketing('/tickets?queue=all')

    expect(await screen.findByText(/tjenesten er utilgjengelig/i)).toBeTruthy()
    expect(screen.queryByText(/ingen saker i denne køen/i)).toBeNull()
  })

  it('opens a case-aware Chat launch from a selected ticket', async () => {
    const { history } = renderTicketing('/tickets?queue=all')

    await screen.findAllByText('TCK-REFUND')
    fireEvent.click(screen.getByRole('button', { name: /åpne i verevon chat|open in verevon chat/i }))

    await waitFor(() => expect(history.get()).toBe('/chat'))
    const launch = JSON.parse(window.sessionStorage.getItem('verevon.chat.pendingLaunch') ?? '{}') as { text?: string }
    expect(launch.text).toContain('TCK-REFUND')
    expect(launch.text).toContain('Refund blocked after delivery issue')
  })
})
