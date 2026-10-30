import { describe, expect, it } from 'vitest'
import { resolveChatWorkPeriod, withChatWorkPeriod } from './chat-work-period'

describe('chat working period', () => {
  const now = new Date(2026, 0, 11, 12)

  it('resolves a previous quarter across a year boundary', () => {
    expect(resolveChatWorkPeriod({ kind: 'last_quarter' }, now)).toEqual({ from: '2025-10-01', to: '2025-12-31' })
  })

  it('adds the chosen range visibly and leaves an unselected turn untouched', () => {
    expect(withChatWorkPeriod('Hva skjedde?', { kind: 'none' }, 'no', now)).toBe('Hva skjedde?')
    expect(withChatWorkPeriod('Hva skjedde?', { kind: 'this_month' }, 'no', now)).toContain('Arbeidsperiode: 2026-01-01 til 2026-01-11')
  })

  it('rejects a reversed custom range', () => {
    expect(resolveChatWorkPeriod({ kind: 'custom', from: '2026-02-01', to: '2026-01-31' }, now)).toBeNull()
  })
})
