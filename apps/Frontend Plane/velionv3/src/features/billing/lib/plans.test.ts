import { describe, expect, it } from 'vitest'
import { isCheckoutActivatingStatus } from '@/features/billing/lib/plans'

describe('isCheckoutActivatingStatus', () => {
  it.each(['succeeded', 'processing', 'charged', 'reserved'])('accepts the activating provider status %s', (status) => {
    expect(isCheckoutActivatingStatus(status)).toBe(true)
  })

  it.each(['created', 'failed', 'cancelled', '', undefined])('rejects the non-activating provider status %s', (status) => {
    expect(isCheckoutActivatingStatus(status)).toBe(false)
  })
})
