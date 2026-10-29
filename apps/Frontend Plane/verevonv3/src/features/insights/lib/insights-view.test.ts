import { describe, expect, it } from 'vitest'
import { buildInsightsOverviewQuery } from '@/features/insights/lib/insights-view'

describe('buildInsightsOverviewQuery', () => {
  const now = new Date('2026-08-05T12:30:00.000Z')

  it('keeps the overview unfiltered when the user selects all recorded activity', () => {
    expect(buildInsightsOverviewQuery('overview', 'all', now)).toEqual({})
  })

  it('uses a gateway-safe surface and explicit UTC time window for a scoped tab', () => {
    expect(buildInsightsOverviewQuery('inbox', '30d', now)).toEqual({
      surface: 'inbox',
      from: '2026-07-06T12:30:00.000Z',
      to: '2026-08-05T12:30:00.000Z',
    })
  })

  it('uses the explicit external analytics surface for provider metric rows', () => {
    expect(buildInsightsOverviewQuery('external_analytics', '7d', now)).toEqual({
      surface: 'external_analytics',
      from: '2026-07-29T12:30:00.000Z',
      to: '2026-08-05T12:30:00.000Z',
    })
  })

  it('uses a scope selector but never a browser-supplied user or organization id', () => {
    expect(buildInsightsOverviewQuery('chat', '7d', now, 'me')).toEqual({
      surface: 'chat',
      scope: 'me',
      from: '2026-07-29T12:30:00.000Z',
      to: '2026-08-05T12:30:00.000Z',
    })
  })

  it('targets the durable Knowledge lifecycle surface alongside its permission-scoped snapshot', () => {
    expect(buildInsightsOverviewQuery('knowledge', 'all', now)).toEqual({ surface: 'knowledge' })
  })
})
