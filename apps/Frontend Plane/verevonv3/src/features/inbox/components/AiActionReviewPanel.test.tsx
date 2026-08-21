// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { AiActionReviewPanel } from './AiActionReviewPanel'

const suggestedClassification = {
  id: 'aiact_1',
  org_id: 'org_demo',
  conversation_id: 'conv_1',
  kind: 'ticket.classification',
  status: 'suggest_ticket',
  payload: {
    confidence: 0.91,
    reason: 'The customer reports a missing delivery and needs carrier follow-up.',
    evidence_message_ids: ['100', '101'],
    suggested_fields: {
      category: 'delivery',
      intent: 'customer_follow_up',
      priority: 'high',
      severity: 'high',
    },
  },
  created_by: 'model-plane',
  created_at: '2026-08-02T12:00:00.000Z',
  updated_at: '2026-08-02T12:00:00.000Z',
}

const suggestedReply = {
  id: 'aiact_reply_1',
  org_id: 'org_demo',
  conversation_id: 'conv_1',
  kind: 'draft.reply',
  status: 'suggested',
  payload: { confidence: 0.88, body_text: 'We have checked your delivery and will update you tomorrow.' },
  created_by: 'model-plane',
  created_at: '2026-08-02T12:00:00.000Z',
  updated_at: '2026-08-02T12:00:00.000Z',
}

const suggestedTicketUpdate = {
  id: 'aiact_update_1',
  org_id: 'org_demo',
  conversation_id: 'conv_1',
  kind: 'ticket.update',
  status: 'suggested',
  payload: {
    ticket_id: 'ticket_1', confidence: 0.84, reason: 'The delivery is now urgent.', evidence_message_ids: ['100'],
    suggested_fields: { priority: 'urgent', severity: 'high', status: 'waiting_customer' },
  },
  created_by: 'model-plane',
  created_at: '2026-08-02T12:00:00.000Z',
  updated_at: '2026-08-02T12:00:00.000Z',
}

const suggestedIncident = {
  id: 'aiact_incident_1',
  org_id: 'org_demo',
  conversation_id: 'conv_1',
  kind: 'incident.create',
  status: 'suggested',
  payload: {
    ticket_id: 'ticket_1', title: 'Checkout errors across web', severity: 'critical', customer_impact: 'Customers cannot complete checkout.',
    confidence: 0.93, reason: 'Multiple reports share the same checkout failure.', evidence_message_ids: ['100', '102'],
  },
  created_by: 'model-plane',
  created_at: '2026-08-03T12:00:00.000Z',
  updated_at: '2026-08-03T12:00:00.000Z',
}

const suggestedProblem = {
  id: 'aiact_problem_1', org_id: 'org_demo', conversation_id: 'conv_1', kind: 'problem.create', status: 'suggested',
  payload: {
    title: 'Checkout dependency instability', summary: 'Several checkout failures share a timeout.', root_cause: 'Gateway timeout observed.',
    confidence: 0.91, reason: 'Multiple reports show the same checkout failure.', evidence_message_ids: ['100', '102'],
  },
  created_by: 'model-plane', created_at: '2026-08-03T12:00:00.000Z', updated_at: '2026-08-03T12:00:00.000Z',
}

function jsonResponse(data: unknown, status = 200) {
  return new Response(JSON.stringify({ data }), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
})

describe('AiActionReviewPanel', () => {
  it('shows and persists the exact editable Incident proposal before approval', async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input)
      if (url.startsWith('/api/v1/inbox/ai-actions?')) return jsonResponse([suggestedIncident])
      if (url.endsWith('/api/v1/inbox/ai-actions/aiact_incident_1/approve') && init?.method === 'POST') return jsonResponse({ ok: true })
      return jsonResponse([])
    })
    vi.stubGlobal('fetch', fetchMock)
    render(() => <AiActionReviewPanel conversationId="conv_1" />)

    expect(await screen.findByText(/foreslått hendelse/i)).toBeTruthy()
    const title = screen.getByRole('textbox', { name: /hendelsestittel.*redigerbar/i })
    fireEvent.input(title, { target: { value: 'Checkout failure across web' } })
    flush()
    fireEvent.click(screen.getByRole('button', { name: /godkjenn/i }))
    await waitFor(() => {
      const approval = fetchMock.mock.calls.find(([input]) => String(input).endsWith('/api/v1/inbox/ai-actions/aiact_incident_1/approve'))
      expect(JSON.parse(String(approval?.[1]?.body))).toEqual({ edited_fields: {
        title: 'Checkout failure across web', severity: 'critical', customer_impact: 'Customers cannot complete checkout.',
      } })
    })
  })

  it('shows and persists an independent editable Problem candidate before approval', async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input)
      if (url.startsWith('/api/v1/inbox/ai-actions?')) return jsonResponse([suggestedProblem])
      if (url.endsWith('/api/v1/inbox/ai-actions/aiact_problem_1/approve') && init?.method === 'POST') return jsonResponse({ ok: true })
      return jsonResponse([])
    })
    vi.stubGlobal('fetch', fetchMock)
    render(() => <AiActionReviewPanel conversationId="conv_1" />)

    expect(await screen.findByText(/foreslått problem/i)).toBeTruthy()
    const summary = screen.getByRole('textbox', { name: /sammendrag.*redigerbar/i })
    fireEvent.input(summary, { target: { value: 'Checkout failures share an upstream timeout.' } })
    flush()
    fireEvent.click(screen.getByRole('button', { name: /godkjenn/i }))
    await waitFor(() => {
      const approval = fetchMock.mock.calls.find(([input]) => String(input).endsWith('/api/v1/inbox/ai-actions/aiact_problem_1/approve'))
      expect(JSON.parse(String(approval?.[1]?.body))).toEqual({ edited_fields: {
        title: 'Checkout dependency instability', summary: 'Checkout failures share an upstream timeout.', root_cause: 'Gateway timeout observed.',
      } })
    })
  })

  it('lets a reviewer inspect and edit an AI ticket update before approval', async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input)
      if (url.startsWith('/api/v1/inbox/ai-actions?')) return jsonResponse([suggestedTicketUpdate])
      if (url.endsWith('/api/v1/inbox/ai-actions/aiact_update_1/approve') && init?.method === 'POST') return jsonResponse({ ok: true })
      return jsonResponse([])
    })
    vi.stubGlobal('fetch', fetchMock)

    render(() => <AiActionReviewPanel conversationId="conv_1" />)
    expect(await screen.findByText(/foreslått saksoppdatering/i)).toBeTruthy()
    expect(screen.getByText('The delivery is now urgent.')).toBeTruthy()
    expect(screen.getByText(/Meldingsreferanser: 100/i)).toBeTruthy()
    const priority = screen.getByRole('textbox', { name: /prioritet.*redigerbar/i })
    fireEvent.input(priority, { target: { value: 'high' } })
    flush()
		const status = screen.getByRole('combobox', { name: /status.*redigerbar/i })
		expect((status as HTMLSelectElement).value).toBe('waiting_customer')
		fireEvent.change(status, { target: { value: 'waiting_team' } })
    flush()
    fireEvent.click(screen.getByRole('button', { name: /godkjenn/i }))
    await waitFor(() => {
      const approval = fetchMock.mock.calls.find(([input]) => String(input).endsWith('/api/v1/inbox/ai-actions/aiact_update_1/approve'))
		expect(JSON.parse(String(approval?.[1]?.body))).toEqual({ edited_fields: { priority: 'high', severity: 'high', status: 'waiting_team' } })
    })
  })

  it('labels a persisted resolution-plan correlation without combining its approvals', async () => {
    const groupedReply = { ...suggestedReply, proposal_group_id: 'resolution_abc123' }
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      if (String(input).startsWith('/api/v1/inbox/ai-actions?')) return jsonResponse([groupedReply])
      return jsonResponse([])
    })
    vi.stubGlobal('fetch', fetchMock)

    render(() => <AiActionReviewPanel conversationId="conv_1" />)
    expect(await screen.findByText(/Del av én løsningsplan/i)).toBeTruthy()
    expect(screen.getAllByRole('button', { name: /Godkjenn/i })).toHaveLength(1)
    expect(screen.getAllByRole('button', { name: /Avvis/i })).toHaveLength(1)
  })

  it('presents a multi-action resolution plan as one scoped bundle with independent decisions', async () => {
    const groupId = 'resolution_multi_123'
    const groupedReply = { ...suggestedReply, proposal_group_id: groupId }
    const groupedUpdate = { ...suggestedTicketUpdate, proposal_group_id: groupId }
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      if (String(input).startsWith('/api/v1/inbox/ai-actions?')) return jsonResponse([groupedReply, groupedUpdate])
      return jsonResponse([])
    })
    vi.stubGlobal('fetch', fetchMock)

    render(() => <AiActionReviewPanel conversationId="conv_1" />)

    const plan = await screen.findByRole('group', { name: /ai resolution plan|ai-løsningsplan/i })
    expect(plan.getAttribute('data-ai-proposal-group')).toBe(groupId)
    expect(plan.textContent).toMatch(/2 independent decisions|2 separate avgjørelser/i)
    expect(plan.textContent).toMatch(/there is no approve-all action|ingen godkjenn-alt-handling/i)
    expect(screen.getAllByRole('button', { name: /approve|godkjenn/i })).toHaveLength(2)
    expect(screen.getAllByRole('button', { name: /reject|avvis/i })).toHaveLength(2)
  })

  it('records the exact reviewed proposal but does not claim execution before a receipt exists', async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input)
      if (url.startsWith('/api/v1/inbox/ai-actions?')) return jsonResponse([suggestedClassification])
      if (url.endsWith('/api/v1/inbox/ai-actions/aiact_1/approve') && init?.method === 'POST') {
        return jsonResponse({ ok: true })
      }
      return jsonResponse([])
    })
    vi.stubGlobal('fetch', fetchMock)

    render(() => <AiActionReviewPanel conversationId="conv_1" />)

    const severity = await screen.findByRole('textbox', { name: /alvorlighetsgrad.*redigerbar/i })
    expect(screen.getByText('The customer reports a missing delivery and needs carrier follow-up.')).toBeTruthy()
    expect(screen.getByText(/Meldingsreferanser: 100, 101/i)).toBeTruthy()
    fireEvent.input(severity, { target: { value: 'critical' } })
    flush()
    fireEvent.click(screen.getByRole('button', { name: /godkjenn/i }))

    await waitFor(() => {
      const approval = fetchMock.mock.calls.find(([input, init]) =>
        String(input).endsWith('/api/v1/inbox/ai-actions/aiact_1/approve') && init?.method === 'POST',
      )
      expect(approval).toBeTruthy()
      expect(JSON.parse(String(approval?.[1]?.body))).toEqual({
        edited_fields: {
          category: 'delivery',
          intent: 'customer_follow_up',
          priority: 'high',
          severity: 'critical',
        },
      })
    })

    expect(await screen.findByText(/godkjenningen er registrert.*venter på verifisert utførelse/i)).toBeTruthy()
    expect(screen.queryByText(/utført.*forfremmet og rutet/i)).toBeNull()
  })

  it('allows a valid AI routing proposal to be changed only to a canonical Ticketing team', async () => {
    const routedSuggestion = {
      ...suggestedClassification,
      payload: {
        ...suggestedClassification.payload,
        suggested_fields: {
          ...suggestedClassification.payload.suggested_fields,
          team_id: 'team_billing',
          team_name: 'Billing',
        },
      },
    }
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input)
      if (url.startsWith('/api/v1/inbox/ai-actions?')) return jsonResponse([routedSuggestion])
      if (url.endsWith('/api/v1/inbox/ai-actions/aiact_1/approve') && init?.method === 'POST') return jsonResponse({ ok: true })
      return jsonResponse([])
    })
    vi.stubGlobal('fetch', fetchMock)

    render(() => (
      <AiActionReviewPanel
        conversationId="conv_1"
        ticketTeams={[
          { id: 'team_billing', org_id: 'org_demo', name: 'Billing', active: true, created_at: '', updated_at: '' },
          { id: 'team_delivery', org_id: 'org_demo', name: 'Delivery', active: true, created_at: '', updated_at: '' },
        ]}
      />
    ))

    const teamSelect = await screen.findByRole('combobox', { name: /team.*redigerbar/i })
    fireEvent.change(teamSelect, { target: { value: 'team_delivery' } })
    flush()
    fireEvent.click(screen.getByRole('button', { name: /godkjenn/i }))

    await waitFor(() => {
      const approval = fetchMock.mock.calls.find(([input, init]) =>
        String(input).endsWith('/api/v1/inbox/ai-actions/aiact_1/approve') && init?.method === 'POST',
      )
      expect(JSON.parse(String(approval?.[1]?.body))).toMatchObject({
        edited_fields: { team_id: 'team_delivery', team_name: 'Delivery' },
      })
    })
  })

  it('keeps a ticket-update routing proposal reviewable only with an active canonical team', async () => {
    const routedTicketUpdate = {
      ...suggestedTicketUpdate,
      payload: {
        ...suggestedTicketUpdate.payload,
        suggested_fields: {
          ...suggestedTicketUpdate.payload.suggested_fields,
          team_id: 'team_billing',
          team_name: 'Billing',
        },
      },
    }
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input)
      if (url.startsWith('/api/v1/inbox/ai-actions?')) return jsonResponse([routedTicketUpdate])
      if (url.endsWith('/api/v1/inbox/ai-actions/aiact_update_1/approve') && init?.method === 'POST') return jsonResponse({ ok: true })
      return jsonResponse([])
    })
    vi.stubGlobal('fetch', fetchMock)

    render(() => (
      <AiActionReviewPanel
        conversationId="conv_1"
        ticketTeams={[
          { id: 'team_billing', org_id: 'org_demo', name: 'Billing', active: true, created_at: '', updated_at: '' },
          { id: 'team_delivery', org_id: 'org_demo', name: 'Delivery', active: true, created_at: '', updated_at: '' },
        ]}
      />
    ))

    const teamSelect = await screen.findByRole('combobox', { name: /team.*redigerbar/i })
    fireEvent.change(teamSelect, { target: { value: 'team_delivery' } })
    flush()
    fireEvent.click(screen.getByRole('button', { name: /godkjenn/i }))

    await waitFor(() => {
      const approval = fetchMock.mock.calls.find(([input, init]) =>
        String(input).endsWith('/api/v1/inbox/ai-actions/aiact_update_1/approve') && init?.method === 'POST',
      )
      expect(JSON.parse(String(approval?.[1]?.body))).toMatchObject({
        edited_fields: { team_id: 'team_delivery', team_name: 'Delivery' },
      })
    })
  })

  it('blocks approval of a stale ticket-update team until a reviewer repairs it', async () => {
    const staleTicketUpdate = {
      ...suggestedTicketUpdate,
      payload: {
        ...suggestedTicketUpdate.payload,
        suggested_fields: {
          ...suggestedTicketUpdate.payload.suggested_fields,
          team_id: 'team_retired',
          team_name: 'Retired Team',
        },
      },
    }
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input)
      if (url.startsWith('/api/v1/inbox/ai-actions?')) return jsonResponse([staleTicketUpdate])
      if (url.endsWith('/api/v1/inbox/ai-actions/aiact_update_1/approve') && init?.method === 'POST') return jsonResponse({ ok: true })
      return jsonResponse([])
    })
    vi.stubGlobal('fetch', fetchMock)

    render(() => (
      <AiActionReviewPanel
        conversationId="conv_1"
        ticketTeams={[{ id: 'team_delivery', org_id: 'org_demo', name: 'Delivery', active: true, created_at: '', updated_at: '' }]}
      />
    ))

    await screen.findByText(/foreslått team er ikke aktivt/i)
    fireEvent.click(screen.getByRole('button', { name: /godkjenn/i }))

    expect((await screen.findByRole('alert')).textContent).toMatch(/velg et aktivt, kanonisk Ticketing-team/i)
    expect(fetchMock.mock.calls.some(([input, init]) =>
      String(input).endsWith('/api/v1/inbox/ai-actions/aiact_update_1/approve') && init?.method === 'POST',
    )).toBe(false)
  })

  it('lets a reviewer repair a stale AI team proposal by choosing an active canonical team', async () => {
    const staleRoutedSuggestion = {
      ...suggestedClassification,
      payload: {
        ...suggestedClassification.payload,
        suggested_fields: {
          ...suggestedClassification.payload.suggested_fields,
          team_id: 'team_retired',
          team_name: 'Retired Team',
        },
      },
    }
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input)
      if (url.startsWith('/api/v1/inbox/ai-actions?')) return jsonResponse([staleRoutedSuggestion])
      if (url.endsWith('/api/v1/inbox/ai-actions/aiact_1/approve') && init?.method === 'POST') return jsonResponse({ ok: true })
      return jsonResponse([])
    })
    vi.stubGlobal('fetch', fetchMock)

    render(() => (
      <AiActionReviewPanel
        conversationId="conv_1"
        ticketTeams={[
          { id: 'team_billing', org_id: 'org_demo', name: 'Billing', active: true, created_at: '', updated_at: '' },
          { id: 'team_delivery', org_id: 'org_demo', name: 'Delivery', active: true, created_at: '', updated_at: '' },
        ]}
      />
    ))

    expect(await screen.findByText(/foreslått team er ikke aktivt/i)).toBeTruthy()
    const teamSelect = screen.getByRole('combobox', { name: /team.*redigerbar/i })
    fireEvent.change(teamSelect, { target: { value: 'team_delivery' } })
    flush()
    fireEvent.click(screen.getByRole('button', { name: /godkjenn/i }))

    await waitFor(() => {
      const approval = fetchMock.mock.calls.find(([input, init]) =>
        String(input).endsWith('/api/v1/inbox/ai-actions/aiact_1/approve') && init?.method === 'POST',
      )
      expect(JSON.parse(String(approval?.[1]?.body))).toMatchObject({
        edited_fields: { team_id: 'team_delivery', team_name: 'Delivery' },
      })
    })
  })

  it('shows a verified ticket receipt only after the canonical action ledger reports execution', async () => {
    let approved = false
    const onTicketActionVerified = vi.fn()
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input)
      if (url.startsWith('/api/v1/inbox/ai-actions?')) {
        return jsonResponse([
          {
            ...suggestedClassification,
            status: approved ? 'executed' : 'suggest_ticket',
          },
        ])
      }
      if (url.endsWith('/api/v1/inbox/ai-actions/aiact_1/approve') && init?.method === 'POST') {
        approved = true
        return jsonResponse({ ok: true })
      }
      return jsonResponse([])
    })
    vi.stubGlobal('fetch', fetchMock)

    render(() => <AiActionReviewPanel conversationId="conv_1" onTicketActionVerified={onTicketActionVerified} />)

    await screen.findByRole('textbox', { name: /alvorlighetsgrad.*redigerbar/i })
    fireEvent.click(screen.getByRole('button', { name: /godkjenn/i }))

    expect(await screen.findByText(/utførelse verifisert.*support-saken er åpnet/i)).toBeTruthy()
    expect(screen.queryByText(/venter på verifisert utførelse/i)).toBeNull()
    expect(onTicketActionVerified).toHaveBeenCalledOnce()
  })

  it('reconciles a false-failure 502 by checking whether the decision was actually recorded', async () => {
    let approved = false
    const onTicketActionVerified = vi.fn()
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input)
      if (url.startsWith('/api/v1/inbox/ai-actions?')) {
        return jsonResponse([
          { ...suggestedClassification, status: approved ? 'executed' : 'suggest_ticket' },
        ])
      }
      if (url.endsWith('/api/v1/inbox/ai-actions/aiact_1/approve') && init?.method === 'POST') {
        // The decision reaches the backend and is durably recorded, but the
        // response itself is lost to a transient gateway error.
        approved = true
        return jsonResponse({ message: 'Bad Gateway' }, 502)
      }
      return jsonResponse([])
    })
    vi.stubGlobal('fetch', fetchMock)

    render(() => <AiActionReviewPanel conversationId="conv_1" onTicketActionVerified={onTicketActionVerified} />)

    await screen.findByRole('textbox', { name: /alvorlighetsgrad.*redigerbar/i })
    fireEvent.click(screen.getByRole('button', { name: /godkjenn/i }))

    expect(await screen.findByText(/utførelse verifisert.*support-saken er åpnet/i)).toBeTruthy()
    expect(screen.queryByRole('alert')).toBeNull()
    expect(onTicketActionVerified).toHaveBeenCalledOnce()
  })

  it('shows a real failure when a decide request errors and the decision genuinely did not go through', async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input)
      if (url.startsWith('/api/v1/inbox/ai-actions?')) return jsonResponse([suggestedClassification])
      if (url.endsWith('/api/v1/inbox/ai-actions/aiact_1/approve') && init?.method === 'POST') {
        return jsonResponse({ message: 'Bad Gateway' }, 502)
      }
      return jsonResponse([])
    })
    vi.stubGlobal('fetch', fetchMock)

    render(() => <AiActionReviewPanel conversationId="conv_1" />)

    await screen.findByRole('textbox', { name: /alvorlighetsgrad.*redigerbar/i })
    fireEvent.click(screen.getByRole('button', { name: /godkjenn/i }))

    expect((await screen.findByRole('alert')).textContent).toMatch(/kunne ikke registrere avgjørelsen/i)
    expect(screen.queryByText(/registrert/i)).toBeNull()
  })

  it('lets an operator edit the exact draft reply before approval', async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input)
      if (url.startsWith('/api/v1/inbox/ai-actions?')) return jsonResponse([suggestedReply])
      if (url.endsWith('/api/v1/inbox/ai-actions/aiact_reply_1/approve') && init?.method === 'POST') return jsonResponse({ ok: true })
      return jsonResponse([])
    })
    vi.stubGlobal('fetch', fetchMock)

    render(() => <AiActionReviewPanel conversationId="conv_1" />)

    const editor = await screen.findByRole('textbox', { name: /svarutkast.*redigerbar/i })
    expect((editor as HTMLTextAreaElement).value).toBe('We have checked your delivery and will update you tomorrow.')
    expect(screen.getByText(/nøyaktig svarutkast/i)).toBeTruthy()
    fireEvent.input(editor, { target: { value: 'A human-reviewed reply.' } })
    flush()
    fireEvent.click(screen.getByRole('button', { name: /godkjenn/i }))
    await waitFor(() => {
      const approval = fetchMock.mock.calls.find(([input]) => String(input).endsWith('/api/v1/inbox/ai-actions/aiact_reply_1/approve'))
      expect(JSON.parse(String(approval?.[1]?.body))).toEqual({ edited_fields: { body_text: 'A human-reviewed reply.' } })
    })
  })

  it('does not present a ledger outage as an empty AI review queue', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse({ message: 'Unavailable' }, 503)))

    render(() => <AiActionReviewPanel conversationId="conv_1" />)

    expect((await screen.findByRole('alert')).textContent).toMatch(/kunne ikke laste ai-forslagene/i)
    expect(screen.queryByText(/ingen ai-forslag venter/i)).toBeNull()
  })
})
