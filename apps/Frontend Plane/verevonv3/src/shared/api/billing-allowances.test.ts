import { describe, expect, it } from 'vitest'
import { allowanceSchema } from './billing-allowances'

const period = { from: '2026-10-01T00:00:00Z', to: '2026-11-01T00:00:00Z', included: 100, carried: 20, granted: 10, used: 30, overage: 0, expired: 0, remaining: 100 }
const report = { policy: { metric: 'ai.metered.requests', name: 'Requests', unit: 'requests', version: 'v1', starts_at: period.from, interval_months: 1, included_units: 100, rollover_cap: 100, rollover_cycles: 1 }, current: period, history: [], as_of: period.from, active: true }
describe('billing allowance response contract', () => {
  it('retains carry-over and grants separately from usage', () => {
    const result = allowanceSchema.parse(report)
    expect(result.current.remaining).toBe(result.current.included + result.current.carried + result.current.granted - result.current.used)
  })
  it('rejects amounts that JavaScript cannot represent exactly', () => {
    expect(allowanceSchema.safeParse({ ...report, current: { ...period, remaining: Number.MAX_SAFE_INTEGER + 1 } }).success).toBe(false)
  })
  it('rejects monetary units in a token/request allowance', () => {
    expect(allowanceSchema.safeParse({ ...report, policy: { ...report.policy, unit: 'usd' } }).success).toBe(false)
  })
})
