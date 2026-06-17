import { describe, expect, it } from 'vitest'
import {
  buildExternalAnalyticsSlots,
  normalizeIntegrationConnections,
  withResourceTimeout,
  type ResourceResult,
} from '@/features/insights/lib/insights-workspace'
import type { InsightConnector } from '@/shared/api/insights-client'
import type { IntegrationConnection } from '@/shared/api/integrations-client'

describe('buildExternalAnalyticsSlots', () => {
  it('keeps GA4 and Search Console unconnected when no connector exists', () => {
    const slots = buildExternalAnalyticsSlots(integrations([]))

    expect(slots).toHaveLength(2)
    expect(slots[0]).toMatchObject({
      connectorId: 'ga4',
      title: 'Google Analytics 4',
      status: 'not_connected',
      statusLabel: 'Not connected',
    })
    expect(slots[0]?.reportConcepts).toEqual(expect.arrayContaining(['dateRanges[]', 'dimensions[]', 'metrics[]']))
    expect(slots[0]?.responseConcepts).toEqual(expect.arrayContaining(['rows[]', 'rowCount']))
    expect(slots[1]).toMatchObject({
      connectorId: 'search-console',
      title: 'Search Console SEO',
      status: 'not_connected',
      statusLabel: 'Not connected',
    })
    expect(slots[1]?.metricConcepts).toEqual(expect.arrayContaining(['queries', 'pages', 'clicks', 'impressions', 'position']))
  })

  it('marks connected Google connectors as report-pending instead of live rows', () => {
    const slots = buildExternalAnalyticsSlots(integrations([
      connection('conn_ga4', 'google-analytics-4', 'Google Analytics 4'),
      connection('conn_gsc', 'google-search-console', 'Google Search Console'),
    ]))

    expect(slots.map((slot) => slot.status)).toEqual(['connected_pending_reports', 'connected_pending_reports'])
    expect(slots[0]?.detail).toContain('needs a GA4 report proxy')
    expect(slots[1]?.detail).toContain('needs a Search Console query proxy')
  })

  it('prefers insight-core connector state when the gateway proxy exists', () => {
    const slots = buildExternalAnalyticsSlots(integrations([
      connection('legacy_ga4', 'google-analytics-4', 'Google Analytics 4'),
    ]), insightConnectors([
      {
        id: 'insight_ga4',
        kind: 'ga4',
        label: 'GA4 property',
        status: 'connected',
        reportShape: {
          dimensions: ['date', 'sessionDefaultChannelGroup'],
          metrics: ['activeUsers', 'conversions'],
          rowsAvailable: false,
        },
      },
      {
        id: 'insight_gsc',
        kind: 'search_console',
        label: 'Search Console property',
        status: 'needs_oauth',
      },
    ]))

    expect(slots[0]?.status).toBe('connected_pending_reports')
    expect(slots[0]?.detail).toContain('connected through insight-core')
    expect(slots[1]?.status).toBe('not_connected')
  })

  it('propagates connector registry outages to both external slots', () => {
    const slots = buildExternalAnalyticsSlots({
      data: [],
      message: 'Integration connector registry is unavailable.',
      state: 'unavailable',
    })

    expect(slots.map((slot) => slot.statusLabel)).toEqual([
      'Connector registry unavailable',
      'Connector registry unavailable',
    ])
  })

  it('falls back when a resource read does not settle', async () => {
    const fallback: ResourceResult<string[]> = {
      data: [],
      message: 'Timed out.',
      state: 'unavailable',
    }

    await expect(withResourceTimeout(new Promise<ResourceResult<string[]>>(() => undefined), fallback, 1))
      .resolves
      .toBe(fallback)
  })

  it('normalizes integration registry response shapes', () => {
    const connectionItem = connection('conn_ga4', 'google-analytics-4', 'Google Analytics 4')

    expect(normalizeIntegrationConnections([connectionItem])).toEqual([connectionItem])
    expect(normalizeIntegrationConnections({ connections: [connectionItem] })).toEqual([connectionItem])
    expect(normalizeIntegrationConnections({ providers: [connectionItem] })).toEqual([])
  })
})

function integrations(connections: IntegrationConnection[]): ResourceResult<IntegrationConnection[]> {
  return {
    data: connections,
    message: connections.length
      ? 'Integration connection registry is live.'
      : 'Integration registry is live, but no analytics connectors are connected.',
    state: connections.length ? 'live' : 'empty',
  }
}

function insightConnectors(connectors: InsightConnector[]): ResourceResult<InsightConnector[]> {
  return {
    data: connectors,
    message: connectors.length
      ? 'Insight-core connector registry is available through the Velion gateway.'
      : 'Insight-core connector registry is live, but no analytics connectors are connected.',
    state: connectors.length ? 'live' : 'empty',
  }
}

function connection(id: string, providerId: string, providerName: string): IntegrationConnection {
  return {
    id,
    providerId,
    providerName,
    status: 'connected',
    createdAt: '2026-06-01T08:00:00.000Z',
  }
}
