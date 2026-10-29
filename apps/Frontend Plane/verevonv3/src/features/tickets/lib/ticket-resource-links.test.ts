import { describe, expect, it } from 'vitest'
import { safeTicketResourceUrl } from './ticket-resource-links'

describe('safeTicketResourceUrl', () => {
  it('allows HTTP(S) destinations', () => {
    expect(safeTicketResourceUrl('https://example.com/order/1')).toBe('https://example.com/order/1')
  })

  it('rejects executable and malformed link schemes', () => {
    expect(safeTicketResourceUrl('javascript:alert(1)')).toBeNull()
    expect(safeTicketResourceUrl('not a url')).toBeNull()
  })
})
