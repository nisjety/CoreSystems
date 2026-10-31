import { describe, expect, it } from 'vitest'
import { isPulseSnapshot } from './pulse-client'
import { sanitizeSearchSnapshot } from './search-client'
import { sanitizeGA4Snapshot } from './ga4-client'
import { sanitizeClaritySnapshot } from './clarity-client'

const site = 'https://triodelab.no/', period = { startDate: '2026-10-01', endDate: '2026-10-22' }
describe('saved analytics availability', () => {
  it.each(['snapshot_pending', 'snapshot_expired', 'authorization_required'])('accepts %s without fabricating metrics or requiring provider reconnection', (reason) => {
    expect(isPulseSnapshot({ site, source: 'google_search_console', status: 'unavailable', reason, period, totals: null, days: [] })).toBe(true)
    expect(sanitizeSearchSnapshot({ site, source: 'google_search_console', status: 'unavailable', reason, period, queries: [], pages: [], opportunities: [], rowLimit: 100, queriesAtLimit: false, pagesAtLimit: false }).reason).toBe(reason)
    expect(sanitizeGA4Snapshot({ site, source: 'google_analytics', status: 'unavailable', reason, period, totals: null, days: [] }).reason).toBe(reason)
    expect(sanitizeClaritySnapshot({ site, source: 'microsoft_clarity', status: 'unavailable', reason, scope: 'clarity_project', window: { numOfDays: 3, timeZone: 'UTC' }, metrics: null }).reason).toBe(reason)
  })
  it('rejects arbitrary reason text from every cached source', () => {
    expect(isPulseSnapshot({ site, source: 'google_search_console', status: 'unavailable', reason: 'token=private', period, totals: null, days: [] })).toBe(false)
  })
})
