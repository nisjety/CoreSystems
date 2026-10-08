import { describe, expect, it } from 'vitest'
import type { UsageAnalytics, UsageBucket } from '@/shared/api/cost-client'
import { usageCsv, usageMoney, usageRows, usageTotals } from './usage-analytics'

const buckets: UsageBucket[] = [
  { day: '2026-10-05', model: 'luna', provider: 'codex', basis: 'subscription', requests: 1, input_tokens: 6241, output_tokens: 5, metered_cost_usd: 0, unmeasured_requests: 0 },
  { day: '2026-10-05', model: 'mini', provider: 'azure', basis: 'metered', requests: 1, input_tokens: 4768, output_tokens: 1, metered_cost_usd: .0007158, unmeasured_requests: 0 },
  { day: '2026-10-06', model: 'mini', provider: 'azure', basis: 'unknown', requests: 1, input_tokens: 0, output_tokens: 0, metered_cost_usd: 0, unmeasured_requests: 1 },
]
describe('usage analytics', () => {
  it('separates request counts, tokens, subscriptions and unknown coverage', () => {
    expect(usageTotals(buckets)).toMatchObject({ requests: 3, input: 11009, output: 6, metered: 1, subscription: 1, unknown: 1, unmeasured: 1, apiCost: .0007158 })
    expect(usageTotals(buckets, 'subscription')).toMatchObject({ requests: 1, input: 6241, apiCost: 0 })
    expect(usageRows(buckets, 'day').map(r => r.requests)).toEqual([2, 1])
    expect(usageRows(buckets, 'model')[0]).toMatchObject({ label: 'mini', requests: 2 })
    expect(usageTotals([])).toMatchObject({ requests: 0, input: 0, output: 0 })
    expect(usageMoney(.0007158)).toBe('< $0.01')
  })
  it('exports scope, effective window and exact estimates without spreadsheet formulas', () => {
    const report: UsageAnalytics = { scope: 'me', reporting_started_at: '2026-10-05T00:00:00Z', from: '2026-10-05T00:00:00Z', to: '2026-10-07T00:00:00Z', timezone: 'UTC', buckets: [...buckets, { ...buckets[1]!, model: '=HYPERLINK("bad")' }] }
    const csv = usageCsv(report, 'metered')
    expect(csv).toContain('0.0007158')
    expect(csv).toContain('"me","2026-10-05T00:00:00Z"')
    expect(csv).toContain('"\'=HYPERLINK(""bad"")"')
    expect(csv).not.toContain('"subscription"')
    expect(csv.split('\r\n')).toHaveLength(3)
  })
})

