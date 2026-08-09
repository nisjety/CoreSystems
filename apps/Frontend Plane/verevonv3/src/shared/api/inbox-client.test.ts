import { afterEach, describe, expect, it, vi } from 'vitest'
import { createDraftReplyProposal, createIncidentProposal, createProblemProposal, listAiActions, listConversations, listOrganizationOutboundIntents, sendReply, submitFeedback, toArticle, toLiveTicket } from './inbox-client'

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('toLiveTicket', () => {
  it('preserves the provider-backed latest message preview', () => {
    const ticket = toLiveTicket({
      id: 'conversation-1',
      org_id: 'org-coresystem',
      inbox_id: 'outlook-main',
      title: 'Quarterly report',
      status: 'open',
      priority: 'normal',
      channel: 'email',
      provider: 'outlook',
      last_message_preview: 'The report is attached for your review.',
      created_at: '2026-07-18T08:00:00.000Z',
      updated_at: '2026-07-18T09:00:00.000Z',
    })

    expect(ticket.lastMessagePreview).toBe('The report is attached for your review.')
  })

  it.each([
    ['google', 'email'],
    ['microsoft', 'email'],
    ['slack', 'slack'],
    ['teams', 'teams'],
    ['discord', 'discord'],
    ['whatsapp', 'whatsapp'],
    ['messenger', 'messenger'],
    ['instagram', 'instagram'],
    ['x', 'x'],
  ])('preserves %s identity on the %s channel', (provider, channel) => {
    const ticket = toLiveTicket({
      id: `conversation-${provider}`,
      org_id: 'org-coresystem',
      inbox_id: `${provider}-main`,
      title: `${provider} conversation`,
      status: 'open',
      priority: 'normal',
      channel,
      provider,
      created_at: '2026-07-18T08:00:00.000Z',
      updated_at: '2026-07-18T09:00:00.000Z',
    })

    expect(ticket).toMatchObject({ provider, channel })
  })
})

describe('toArticle', () => {
  it('preserves only safe attachment metadata from conversation-core', () => {
    const article = toArticle({
      id: 'message-1', conversation_id: 'conversation-1', direction: 'inbound', sender_type: 'customer',
      body_text: 'The document is attached.', internal: false, occurred_at: '2026-08-03T08:00:00.000Z', created_at: '2026-08-03T08:00:00.000Z',
      attachments: [{ id: 'attachment-1', filename: 'report.pdf', mime_type: 'application/pdf', size_bytes: 42_000 }],
    })

    expect(article.attachments).toEqual([{ id: 'attachment-1', filename: 'report.pdf', mimeType: 'application/pdf', sizeBytes: 42_000 }])
  })
})

describe('listConversations', () => {
  it('forwards a canonical provider channel to the server query', async () => {
    const fetchMock = vi.fn(async () => jsonResponse([]))
    vi.stubGlobal('fetch', fetchMock)

    await listConversations('org-coresystem', { channel: 'slack', limit: 50 })

    const [url] = fetchMock.mock.calls[0] as unknown as [string, RequestInit]
    expect(new URL(url, 'http://verevon.local').searchParams.get('channel')).toBe('slack')
  })

  it('forwards the pagination cursor and derives the next page from the last conversation', async () => {
    const rows = [
      {
        id: 'conversation-older', org_id: 'org-coresystem', inbox_id: 'inbox-email', title: 'Older mail',
        status: 'open', priority: 'normal', channel: 'email', provider: 'microsoft',
        created_at: '2026-07-10T08:00:00.000Z', updated_at: '2026-07-10T09:00:00.000Z',
      },
    ]
    const fetchMock = vi.fn(async () => jsonResponse(rows))
    vi.stubGlobal('fetch', fetchMock)

    const result = await listConversations('org-coresystem', {
      limit: 1,
      cursorUpdated: '2026-07-11T09:00:00.000Z',
      cursorId: 'conversation-newer',
    })

    const [url] = fetchMock.mock.calls[0] as unknown as [string, RequestInit]
    const params = new URL(url, 'http://verevon.local').searchParams
    expect(params.get('cursor_updated')).toBe('2026-07-11T09:00:00.000Z')
    expect(params.get('cursor_id')).toBe('conversation-newer')
    expect(result.nextCursor).toEqual({
      updated: '2026-07-10T09:00:00.000Z',
      id: 'conversation-older',
    })
  })
})

describe('listOrganizationOutboundIntents', () => {
  it('uses only exact canonical receipt filters', async () => {
    const fetchMock = vi.fn(async () => jsonResponse([]))
    vi.stubGlobal('fetch', fetchMock)

    await listOrganizationOutboundIntents('org-coresystem', { status: 'submitted', provider: 'whatsapp', deliveryStatus: 'unconfirmed', limit: 25 })

    const [url] = fetchMock.mock.calls[0] as unknown as [string, RequestInit]
    const params = new URL(url, 'http://verevon.local').searchParams
    expect(params.get('status')).toBe('submitted')
    expect(params.get('provider')).toBe('whatsapp')
    expect(params.get('delivery_status')).toBe('unconfirmed')
    expect(params.get('limit')).toBe('25')
  })
})

describe('listAiActions', () => {
  it('requests the explicit all-status ledger for reviewable ticket proposals and durable receipts', async () => {
    const fetchMock = vi.fn(async () => jsonResponse([]))
    vi.stubGlobal('fetch', fetchMock)

    await listAiActions({ conversationId: 'conversation-1', status: 'all' })

    const [url] = fetchMock.mock.calls[0] as unknown as [string, RequestInit]
    const params = new URL(url, 'http://verevon.local').searchParams
    expect(params.get('conversation_id')).toBe('conversation-1')
    expect(params.get('status')).toBe('all')
  })
})

describe('AI proposal grouping', () => {
  it('sends a bounded resolution-plan correlation key without giving the client a status or actor field', async () => {
    const fetchMock = vi.fn(async () => jsonResponse({ id: 'aiact_1' }, 201))
    vi.stubGlobal('fetch', fetchMock)

    await createDraftReplyProposal({
      conversationId: 'conversation-1',
      bodyText: 'Thanks, we are checking.',
      proposalGroupId: 'resolution_abc123',
    })

    const [url, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit]
    expect(url).toBe('/api/v1/inbox/ai-actions')
    expect(JSON.parse(String(init.body))).toEqual({
      conversation_id: 'conversation-1',
      body_text: 'Thanks, we are checking.',
      proposal_group_id: 'resolution_abc123',
    })
  })

  it('sends a bounded incident proposal without lifecycle or ownership controls', async () => {
    const fetchMock = vi.fn(async () => jsonResponse({ id: 'aiact_incident_1' }, 201))
    vi.stubGlobal('fetch', fetchMock)

    await createIncidentProposal({
      conversationId: 'conversation-1',
      ticketId: 'ticket-1',
      title: 'Checkout failures',
      severity: 'critical',
      customerImpact: 'Customers cannot complete checkout.',
      confidence: 0.93,
      reason: 'Several customers report checkout failure.',
      evidenceMessageIds: ['message-1'],
      proposalGroupId: 'resolution_incident123',
    })

    const [url, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit]
    expect(url).toBe('/api/v1/inbox/ai-actions')
    expect(JSON.parse(String(init.body))).toEqual({
      conversation_id: 'conversation-1',
      proposal_group_id: 'resolution_incident123',
      ticket_id: 'ticket-1',
      title: 'Checkout failures',
      severity: 'critical',
      customer_impact: 'Customers cannot complete checkout.',
      confidence: 0.93,
      reason: 'Several customers report checkout failure.',
      evidence_message_ids: ['message-1'],
      kind: 'incident.create',
    })
  })

  it('sends a root-cause candidate without an incident link, owner, or lifecycle control', async () => {
    const fetchMock = vi.fn(async () => jsonResponse({ id: 'aiact_problem_1' }, 201))
    vi.stubGlobal('fetch', fetchMock)
    await createProblemProposal({
      conversationId: 'conversation-1', title: 'Checkout dependency instability', summary: 'Several checkout failures share a timeout.', rootCause: 'Gateway timeout observed.',
      confidence: 0.91, reason: 'Three messages show the same failure.', evidenceMessageIds: ['message-1'], proposalGroupId: 'resolution_problem123',
    })
    const [url, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit]
    expect(url).toBe('/api/v1/inbox/ai-actions')
    expect(JSON.parse(String(init.body))).toEqual({
      conversation_id: 'conversation-1', proposal_group_id: 'resolution_problem123', title: 'Checkout dependency instability', summary: 'Several checkout failures share a timeout.',
      root_cause: 'Gateway timeout observed.', confidence: 0.91, reason: 'Three messages show the same failure.', evidence_message_ids: ['message-1'], kind: 'problem.create',
    })
  })
})

describe('sendReply', () => {
  it('includes the caller idempotency key in the JSON body forwarded for signing', async () => {
    const fetchMock = vi.fn(async () => jsonResponse({
      id: 'message-1',
      conversation_id: 'conversation/1',
      direction: 'outbound',
      sender_type: 'agent',
      body_text: 'The replacement is submitted.',
      internal: false,
      occurred_at: '2026-07-13T14:00:00Z',
      created_at: '2026-07-13T14:00:00Z',
    }))
    vi.stubGlobal('fetch', fetchMock)

    await sendReply(
      'org-acme',
      'conversation/1',
      'The replacement is submitted.',
      false,
      'manual-reply-1234567890',
    )

    const [url, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit]
    expect(url).toBe('/api/v1/inbox/conversations/conversation%2F1/messages')
    expect(init.method).toBe('POST')
    expect(new Headers(init.headers).get('x-verevon-org-id')).toBe('org-acme')
    expect(JSON.parse(String(init.body))).toEqual({
      body_text: 'The replacement is submitted.',
      idempotency_key: 'manual-reply-1234567890',
      internal: false,
    })
  })
})

describe('submitFeedback', () => {
  it('posts the note to the feedback route with a fresh idempotency key and the org header', async () => {
    const fetchMock = vi.fn(async () => jsonResponse({
      id: 'conversation-feedback-1',
      org_id: 'org-acme',
      inbox_id: 'inbox-pilot-feedback',
      title: 'Feedback: The knowledge tab spinner never resolves.',
      status: 'open',
      priority: 'normal',
      channel: 'pilot-feedback',
      tags: ['pilot-feedback'],
      created_at: '2026-07-20T09:00:00.000Z',
      updated_at: '2026-07-20T09:00:00.000Z',
    }, 201))
    vi.stubGlobal('fetch', fetchMock)

    await submitFeedback('org-acme', {
      bodyText: 'The knowledge tab spinner never resolves.',
      fromName: 'Ada',
      fromEmail: 'ada@example.com',
    })

    const [url, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit]
    expect(url).toBe('/api/v1/inbox/feedback')
    expect(init.method).toBe('POST')
    expect(new Headers(init.headers).get('x-verevon-org-id')).toBe('org-acme')
    const parsedBody = JSON.parse(String(init.body))
    expect(parsedBody).toMatchObject({
      body_text: 'The knowledge tab spinner never resolves.',
      from_name: 'Ada',
      from_email: 'ada@example.com',
    })
    expect(typeof parsedBody.idempotency_key).toBe('string')
    expect(parsedBody.idempotency_key.length).toBeGreaterThan(0)
  })

  it('mints a different idempotency key for every call, so retries never merge into one ticket', async () => {
    const fetchMock = vi.fn(async () => jsonResponse({ id: 'conversation-feedback-1' }, 201))
    vi.stubGlobal('fetch', fetchMock)

    await submitFeedback('org-acme', { bodyText: 'First note.' })
    await submitFeedback('org-acme', { bodyText: 'Second note.' })

    const [, firstInit] = fetchMock.mock.calls[0] as unknown as [string, RequestInit]
    const [, secondInit] = fetchMock.mock.calls[1] as unknown as [string, RequestInit]
    const firstKey = JSON.parse(String(firstInit.body)).idempotency_key
    const secondKey = JSON.parse(String(secondInit.body)).idempotency_key
    expect(firstKey).not.toBe(secondKey)
  })
})

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify({ data }), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}
