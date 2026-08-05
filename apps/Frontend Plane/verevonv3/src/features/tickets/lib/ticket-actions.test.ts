import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  executeTicketChecklistCreate,
  executeTicketChecklistItemUpdate,
  executeTicketCreate,
  executeTicketMacro,
  ticketActionForPatch,
} from './ticket-actions'

function jsonResponse(data: unknown) {
  return new Response(JSON.stringify({ data }), {
    headers: { 'Content-Type': 'application/json' },
  })
}

afterEach(() => vi.unstubAllGlobals())

describe('ticketActionForPatch', () => {
  it('uses the dedicated resolve contract for a pure resolution transition', () => {
    expect(ticketActionForPatch('ticket-1', { status: 'resolved' })).toEqual({
      actionId: 'tickets.resolve',
      input: { ticketId: 'ticket-1' },
    })
  })

  it('uses the ordinary update contract to reopen a terminal ticket', () => {
    expect(ticketActionForPatch('ticket-1', { status: 'open' })).toEqual({
      actionId: 'tickets.update',
      input: { ticketId: 'ticket-1', status: 'open' },
    })
  })

  it('uses the assignment contract for owner and team changes', () => {
    expect(ticketActionForPatch('ticket-1', {
      assignee_user_id: 'user-1',
      assignee_name: 'Ada Lovelace',
      team_id: 'team-support',
      team_name: 'Support',
    })).toEqual({
      actionId: 'tickets.assign',
      input: {
        ticketId: 'ticket-1',
        assigneeUserId: 'user-1',
        assigneeName: 'Ada Lovelace',
        teamId: 'team-support',
        teamName: 'Support',
      },
    })
  })

  it('keeps snooze in the update contract with an explicit wake time', () => {
    expect(ticketActionForPatch('ticket-1', {
      status: 'snoozed',
      snoozed_until: '2026-08-03T12:00:00.000Z',
    })).toEqual({
      actionId: 'tickets.update',
      input: {
        ticketId: 'ticket-1',
        status: 'snoozed',
        snoozedUntil: '2026-08-03T12:00:00.000Z',
      },
    })
  })

  it('keeps a team-visible follow-up separate from the SLA due date in the audited update contract', () => {
    expect(ticketActionForPatch('ticket-1', {
      follow_up_at: '2026-08-04T12:00:00.000Z',
    })).toEqual({
      actionId: 'tickets.update',
      input: {
        ticketId: 'ticket-1',
        followUpAt: '2026-08-04T12:00:00.000Z',
      },
    })
  })

  it('rejects mixed assignment and lifecycle changes instead of partially applying a ticket update', () => {
    expect(() => ticketActionForPatch('ticket-1', {
      status: 'waiting_team',
      assignee_user_id: 'user-1',
    })).toThrow(/one action at a time/i)
  })
})

describe('executeTicketCreate', () => {
  it('creates through the action gateway and rereads the authoritative ticket', async () => {
    const fetchMock = vi.fn()
    vi.stubGlobal('fetch', fetchMock)
    fetchMock.mockResolvedValueOnce(jsonResponse({
      actionId: 'tickets.create',
      runId: 'run_ticket_create',
      status: 'completed',
      auditId: 'audit_ticket_create',
      eventStream: '',
      ticketId: 'ticket_new',
    }))
    fetchMock.mockResolvedValueOnce(jsonResponse({
      id: 'ticket_new',
      org_id: 'org-1',
      conversation_id: 'conv-1',
      ticket_key: 'TCK-1',
      status: 'open',
      priority: 'high',
      severity: 'high',
      source: 'manual',
      created_at: '2026-08-02T12:00:00.000Z',
      updated_at: '2026-08-02T12:00:00.000Z',
    }))

    const created = await executeTicketCreate({ type: 'human', userId: 'user-1', orgId: 'org-1' }, {
      conversation_id: 'conv-1',
      priority: 'high',
      severity: 'high',
      category: 'delivery',
      intent: 'customer_follow_up',
    })

    expect(created.id).toBe('ticket_new')
    expect(fetchMock.mock.calls[0]?.[0]).toBe('/api/v1/actions/execute')
    expect(JSON.parse(String(fetchMock.mock.calls[0]?.[1]?.body))).toEqual({
      actionId: 'tickets.create',
      input: {
        conversationId: 'conv-1',
        priority: 'high',
        severity: 'high',
        workType: 'customer_case',
        category: 'delivery',
        intent: 'customer_follow_up',
      },
    })
    expect(fetchMock.mock.calls[1]?.[0]).toBe('/api/v1/tickets/ticket_new')
  })
})

describe('audited ticket work helpers', () => {
  const ticket = {
    id: 'ticket-1', org_id: 'org-1', conversation_id: 'conv-1', ticket_key: 'TCK-1',
    status: 'open', priority: 'normal', severity: 'medium', source: 'manual',
    created_at: '2026-08-02T12:00:00.000Z', updated_at: '2026-08-02T12:00:00.000Z',
  } as const
  const actor = { type: 'human' as const, userId: 'user-1', orgId: 'org-1' }

  it('executes macros and checklist changes through actions before rereading the ticket', async () => {
    const fetchMock = vi.fn()
    vi.stubGlobal('fetch', fetchMock)
    for (const actionId of ['tickets.run_macro', 'tickets.create_checklist', 'tickets.update_checklist_item']) {
      fetchMock.mockResolvedValueOnce(jsonResponse({ actionId, runId: `run_${actionId}`, status: 'completed', auditId: `audit_${actionId}`, eventStream: '' }))
      fetchMock.mockResolvedValueOnce(jsonResponse(ticket))
    }

    await executeTicketMacro(actor, ticket, 'macro-1')
    await executeTicketChecklistCreate(actor, ticket, { name: 'Resolution', items: ['Confirm owner'] })
    await executeTicketChecklistItemUpdate(actor, ticket, { checklistId: 'checklist-1', itemId: 'item-1', completed: true })

    const executions = fetchMock.mock.calls
      .filter(([url]) => url === '/api/v1/actions/execute')
      .map(([, init]) => JSON.parse(String((init as RequestInit).body)))
    expect(executions).toEqual([
      { actionId: 'tickets.run_macro', input: { ticketId: 'ticket-1', macroId: 'macro-1' } },
      { actionId: 'tickets.create_checklist', input: { ticketId: 'ticket-1', name: 'Resolution', items: ['Confirm owner'] } },
      { actionId: 'tickets.update_checklist_item', input: { ticketId: 'ticket-1', checklistId: 'checklist-1', itemId: 'item-1', completed: true } },
    ])
    expect(fetchMock.mock.calls.filter(([url]) => url === '/api/v1/tickets/ticket-1')).toHaveLength(3)
  })
})
