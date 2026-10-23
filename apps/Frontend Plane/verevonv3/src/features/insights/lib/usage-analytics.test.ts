import { describe, expect, it } from 'vitest'
import type { UsageAnalytics, UsageBucket } from '@/shared/api/cost-client'
import { cacheHitRate, cacheMissTotals, cachePercent, usageCsv, usageMoney, usageRows, usageTotals } from './usage-analytics'

const noCache = { cache_read_input_tokens: 0, cache_creation_input_tokens: 0, cache_reported_input_tokens: 0 }
const buckets: UsageBucket[] = [
  { day: '2026-10-05', model: 'luna', provider: 'codex', basis: 'subscription', requests: 1, input_tokens: 6241, output_tokens: 5, metered_cost_usd: 0, unmeasured_requests: 0, ...noCache },
  { day: '2026-10-05', model: 'mini', provider: 'azure', basis: 'metered', requests: 1, input_tokens: 4768, output_tokens: 1, metered_cost_usd: .0007158, unmeasured_requests: 0, ...noCache },
  { day: '2026-10-06', model: 'mini', provider: 'azure', basis: 'unknown', requests: 1, input_tokens: 0, output_tokens: 0, metered_cost_usd: 0, unmeasured_requests: 1, ...noCache },
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
    const report: UsageAnalytics = { scope: 'me', reporting_started_at: '2026-10-05T00:00:00Z', from: '2026-10-05T00:00:00Z', to: '2026-10-07T00:00:00Z', timezone: 'UTC', cache_breakdown: { miss_reasons: [], advisory: true }, buckets: [...buckets, { ...buckets[1]!, model: '=HYPERLINK("bad")' }] }
    const csv = usageCsv(report, 'metered')
    expect(csv).toContain('0.0007158')
    expect(csv).toContain('"me","2026-10-05T00:00:00Z"')
    expect(csv).toContain('"\'=HYPERLINK(""bad"")"')
    expect(csv).not.toContain('"subscription"')
    expect(csv.split('\r\n')).toHaveLength(3)
  })
  it('weights the cache hit rate by input tokens and says nothing when no cache activity was reported', () => {
    const cached: UsageBucket[] = [
      { ...buckets[1]!, model: 'sonnet', input_tokens: 1000, cache_read_input_tokens: 800, cache_creation_input_tokens: 0, cache_reported_input_tokens: 1000 },
      { ...buckets[1]!, model: 'sonnet', day: '2026-10-06', input_tokens: 3000, cache_read_input_tokens: 0, cache_creation_input_tokens: 2500, cache_reported_input_tokens: 3000 },
      { ...buckets[1]!, model: 'plain', input_tokens: 9000 },
    ]
    const total = usageTotals(cached)
    expect(total).toMatchObject({ cacheRead: 800, cacheWrite: 2500, cacheReported: 4000 })
    expect(cacheHitRate(total)).toBe(0.2)
    expect(cachePercent(cacheHitRate(total))).toBe('20.0%')
    const byModel = Object.fromEntries(usageRows(cached, 'model').map(r => [r.label, cacheHitRate(r)]))
    expect(byModel).toEqual({ sonnet: 0.2, plain: null })
    expect(cachePercent(cacheHitRate(usageTotals(buckets)))).toBe('—')
  })
  it('totals miss reasons largest first and shows none for a subscription-only view', () => {
    const misses = [
      { day: '2026-10-05', model: 'm', provider: 'p', reason: 'ttl_expired', requests: 2 },
      { day: '2026-10-06', model: 'm', provider: 'p', reason: 'ttl_expired', requests: 1 },
      { day: '2026-10-06', model: 'm', provider: 'p', reason: 'prefix_changed', requests: 5 },
    ]
    expect(cacheMissTotals(misses)).toEqual([{ reason: 'prefix_changed', requests: 5 }, { reason: 'ttl_expired', requests: 3 }])
    expect(cacheMissTotals(misses, 'subscription')).toEqual([])
  })
  it('exports the cache columns with a blank hit rate where nothing was reported', () => {
    const report: UsageAnalytics = { scope: 'me', reporting_started_at: null, from: null, to: '2026-10-07T00:00:00Z', timezone: 'UTC', cache_breakdown: { miss_reasons: [], advisory: true },
      buckets: [{ ...buckets[1]!, cache_read_input_tokens: 750, cache_creation_input_tokens: 10, cache_reported_input_tokens: 1000 }, buckets[1]!] }
    const lines = usageCsv(report).split('\r\n')
    expect(lines[0]).toContain('"Cache hit rate"')
    expect(lines[1]).toMatch(/,"750","10","0.75"$/)
    expect(lines[2]).toMatch(/,"0","0",""$/)
  })
})

