import { describe, expect, it } from 'vitest'
import type { SupportTicket } from '@/shared/api/tickets-client'
import { deriveTicketRepeatSignals } from './ticket-repeat-signals'

function ticket(overrides: Partial<SupportTicket>): SupportTicket {
  return {
    id: 'ticket-default',
    org_id: 'org-demo',
    conversation_id: 'conversation-default',
    ticket_key: 'TCK-DEFAULT',
    status: 'open',
    work_type: 'customer_case',
    priority: 'normal',
    severity: 'medium',
    source: 'manual',
    created_at: '2026-08-03T09:00:00.000Z',
    updated_at: '2026-08-03T09:00:00.000Z',
    ...overrides,
  }
}

describe('deriveTicketRepeatSignals', () => {
  it('groups only active tickets with the same canonical category, intent, and work type', () => {
    const signals = deriveTicketRepeatSignals([
      ticket({ id: 'ticket-1', ticket_key: 'TCK-1', category: 'Refund ', intent: 'refund_follow_up' }),
      ticket({ id: 'ticket-2', ticket_key: 'TCK-2', category: 'refund', intent: 'refund_follow_up' }),
      ticket({ id: 'ticket-3', ticket_key: 'TCK-3', category: 'refund', intent: 'refund_follow_up', work_type: 'incident' }),
      ticket({ id: 'ticket-4', ticket_key: 'TCK-4', category: 'refund', intent: 'refund_follow_up', status: 'resolved' }),
      ticket({ id: 'ticket-5', ticket_key: 'TCK-5', category: 'refund' }),
    ])

    expect(signals).toEqual([{
      category: 'refund',
      intent: 'refund_follow_up',
      workType: 'customer_case',
      count: 2,
      ticketKeys: ['TCK-1', 'TCK-2'],
    }])
  })

  it('orders the largest repeated signal first and bounds its evidence list', () => {
    const signals = deriveTicketRepeatSignals([
      ...Array.from({ length: 4 }, (_, index) => ticket({ id: `refund-${index}`, ticket_key: `TCK-R${index}`, category: 'refund', intent: 'refund_follow_up' })),
      ...Array.from({ length: 2 }, (_, index) => ticket({ id: `delivery-${index}`, ticket_key: `TCK-D${index}`, category: 'delivery', intent: 'carrier_follow_up' })),
    ])

    expect(signals).toEqual([
      { category: 'refund', intent: 'refund_follow_up', workType: 'customer_case', count: 4, ticketKeys: ['TCK-R0', 'TCK-R1', 'TCK-R2'] },
      { category: 'delivery', intent: 'carrier_follow_up', workType: 'customer_case', count: 2, ticketKeys: ['TCK-D0', 'TCK-D1'] },
    ])
  })
})
