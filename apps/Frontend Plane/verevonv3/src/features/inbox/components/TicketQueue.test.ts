import { describe, expect, it } from 'vitest'
import { ticketCustomerLabel, ticketDisplayTitle } from '@/features/inbox/components/TicketQueue'

describe('ticketDisplayTitle', () => {
  it('shows a Teams counterpart name without an email reply prefix', () => {
    expect(ticketDisplayTitle({ channel: 'teams', title: 'Robert Røsten' })).toBe('Robert Røsten')
  })

  it('shows the Teams counterpart name instead of their email address', () => {
    expect(ticketCustomerLabel({
      channel: 'teams',
      customer: { id: 1, firstname: 'Robert', lastname: 'Røsten', email: 'robert@example.com' },
    })).toBe('Robert Røsten')
  })

  it('keeps the reply prefix for email subjects', () => {
    expect(ticketDisplayTitle({ channel: 'email', title: 'Quarterly report' })).toBe('Re: Quarterly report')
    expect(ticketDisplayTitle({ channel: 'email', title: 'Re: Quarterly report' })).toBe('Re: Quarterly report')
  })
})
