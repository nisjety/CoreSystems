// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { AssistMessage } from '@/features/inbox/lib/inbox-ai'
import type { SupportTicket } from '@/shared/api/tickets-client'
import { buildTicketChatPrompt, launchTicketAssistant } from './ticket-chat-launch'

const ticket: SupportTicket = {
  id: 'ticket-1',
  org_id: 'org-1',
  conversation_id: 'conv-1',
  ticket_key: 'TCK-001',
  status: 'waiting_customer',
  priority: 'high',
  severity: 'medium',
  category: 'refund',
  intent: 'refund_follow_up',
  assignee_name: 'Support Team',
  source: 'ai',
  conversation: {
    id: 'conv-1', org_id: 'org-1', inbox_id: 'support', title: 'Refund request', status: 'open', priority: 'high', channel: 'email',
    contact: { name: 'Maya Solberg', email: 'maya@example.com' },
    last_message_preview: 'Please refund my order 1001.',
    created_at: '2026-08-01T10:00:00.000Z', updated_at: '2026-08-02T10:00:00.000Z',
  },
  created_at: '2026-08-01T10:00:00.000Z',
  updated_at: '2026-08-02T10:00:00.000Z',
}

beforeEach(() => {
  vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({
    data: { actionId: 'tickets.record_chat_handoff', runId: 'handoff_1', status: 'completed' },
  }), { headers: { 'Content-Type': 'application/json' } })))
})

afterEach(() => {
  window.sessionStorage.clear()
  vi.unstubAllGlobals()
})

describe('ticket chat launch', () => {
  it('builds a structured, minimal case prompt without carrying raw contact details or message text', () => {
    const prompt = buildTicketChatPrompt(ticket)

    expect(prompt).toContain('TCK-001')
    expect(prompt).toContain('Refund request')
    expect(prompt).toContain('waiting_customer')
    expect(prompt).toContain('no ticket-side reply editor')
    expect(prompt).toContain('Suggest next action')
    expect(prompt).not.toContain('maya@example.com')
    expect(prompt).not.toContain('Please refund my order 1001.')
  })

  it('stores the explicit case prompt as a deliberate Chat launch', async () => {
    await launchTicketAssistant(ticket, { type: 'human', orgId: 'org-1', userId: 'user-1' })

    const payload = JSON.parse(window.sessionStorage.getItem('verevon.chat.pendingLaunch') ?? '{}') as {
      startNewThread?: boolean
      supportHandoff?: { conversationId?: string; orgId?: string; userId?: string }
      text?: string
    }
    expect(payload.text).toContain('TCK-001')
    expect(payload.startNewThread).toBe(true)
    expect(payload.supportHandoff).toEqual({ conversationId: 'conv-1', orgId: 'org-1', userId: 'user-1' })
    const executeCall = vi.mocked(fetch).mock.calls
      .find(([url, init]) => String(url) === '/api/v1/actions/execute' && init?.method === 'POST')
    expect(executeCall).toBeTruthy()
    const callBody = JSON.parse(String(executeCall?.[1]?.body ?? '{}')) as { actionId?: string; idempotencyKey?: string; input?: unknown }
    expect(callBody).toMatchObject({
      actionId: 'tickets.record_chat_handoff',
      idempotencyKey: expect.any(String),
      input: { ticketId: 'ticket-1' },
    })
  })

  it('includes only the bounded permission-aware conversation evidence when supplied', () => {
    const messages: AssistMessage[] = [{
      agent: false,
      from: 'Maya Solberg',
      body: 'The verification email link returns an error.',
    }]
    const prompt = buildTicketChatPrompt(ticket, messages)

    expect(prompt).toContain('PERMISSION-SCOPED CONVERSATION EVIDENCE')
    expect(prompt).toContain('The verification email link returns an error.')
    expect(prompt).not.toContain('maya@example.com')
  })
})
