import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  getInsightsOverview,
  listInsightConnectors,
  normalizeInsightConnector,
  normalizeInsightOverview,
} from '@/shared/api/insights-client'

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('insights-client', () => {
  it('calls the gateway connectors route with no org header and no internal service keys', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse([])))

    await listInsightConnectors()

    const [url, init] = vi.mocked(fetch).mock.calls[0]!
    const headers = init?.headers as Headers

    expect(String(url)).toBe('/api/v1/insights/connectors')
    // The gateway resolves the org from the session; the SPA must NOT send a
    // client-supplied org header or any internal service key.
    expect(headers.has('x-verevon-org-id')).toBe(false)
    expect(headers.has('x-internal-api-key')).toBe(false)
    expect(headers.has('x-api-key')).toBe(false)
    // Same-origin, cookie-authenticated request.
    expect(init?.credentials).toBe('include')
  })

  it('adapts Insight Core connector slots at the SPA boundary', async () => {
    const slots = [
      { type: 'social-core', surface: 'social', display_name: 'Verevon Social', status: 'native' },
      { type: 'conversation-core', surface: 'inbox', display_name: 'Verevon Inbox', status: 'native' },
      { type: 'model-plane-agents', surface: 'agents', display_name: 'Verevon Agents', status: 'planned' },
      { type: 'google_analytics_4', surface: 'external_analytics', display_name: 'Google Analytics 4', status: 'not_connected' },
    ]
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse(slots)))

    const result = await listInsightConnectors()

    expect(result).toEqual([
      { id: 'social-core', kind: 'social', label: 'Verevon Social', status: 'native' },
      { id: 'conversation-core', kind: 'inbox', label: 'Verevon Inbox', status: 'native' },
      { id: 'model-plane-agents', kind: 'agents', label: 'Verevon Agents', status: 'planned' },
      { id: 'google_analytics_4', kind: 'external_analytics', label: 'Google Analytics 4', status: 'not_connected' },
    ])
    expect(result).toHaveLength(4)
    expect(result[0]).toMatchObject({ id: 'social-core', kind: 'social', status: 'native' })
    expect(result[3]).toMatchObject({ id: 'google_analytics_4', kind: 'external_analytics', status: 'not_connected' })
  })

  it('calls the gateway overview route with no org header and no internal service keys', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse({ scorecards: [], surfaces: [], generated_at: '', source_count: 0 })))

    await getInsightsOverview()

    const [url, init] = vi.mocked(fetch).mock.calls[0]!
    const headers = init?.headers as Headers

    expect(String(url)).toBe('/api/v1/insights/overview')
    // IDOR-clean: the gateway resolves the org from the session.
    expect(headers.has('x-verevon-org-id')).toBe(false)
    expect(headers.has('x-internal-api-key')).toBe(false)
    expect(init?.credentials).toBe('include')
  })

  it('scopes an overview through gateway-owned surface and time filters only', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse({ scorecards: [], surfaces: [], generated_at: '', source_count: 0 })))

    await getInsightsOverview({
      surface: 'inbox',
      from: '2026-08-01T00:00:00.000Z',
      to: '2026-08-05T00:00:00.000Z',
    })

    const [url, init] = vi.mocked(fetch).mock.calls[0]!
    const headers = init?.headers as Headers

    expect(String(url)).toBe('/api/v1/insights/overview?surface=inbox&from=2026-08-01T00%3A00%3A00.000Z&to=2026-08-05T00%3A00%3A00.000Z')
    // Scope stays server-owned: only the allowed reporting filters travel from
    // the browser, never an organization identity or an internal credential.
    expect(headers.has('x-verevon-org-id')).toBe(false)
    expect(headers.has('x-internal-api-key')).toBe(false)
  })

  it('normalizes the gateway overview into real camelCase scorecards', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        jsonResponse({
          generated_at: '2026-06-20T10:00:00Z',
          source_count: 2,
          scorecards: [
            { id: 'inbox.a', label: 'inbox a', surface: 'inbox', metric: 'a', value: 4, unit: 'count', source: 'conversation-core', event_count: 2, aggregation: 'sum_of_recorded_events', definition: 'Two recorded events.' },
            { id: 'social.b', label: 'social b', surface: 'social', metric: 'b', value: 2.5, unit: '', source: '', event_count: 1, aggregation: 'unverified' },
          ],
        }),
      ),
    )

    const overview = await getInsightsOverview()

    expect(overview.generatedAt).toBe('2026-06-20T10:00:00Z')
    expect(overview.sourceCount).toBe(2)
    expect(overview.scorecards).toHaveLength(2)
    expect(overview.scorecards[0]).toEqual({
      id: 'inbox.a',
      label: 'inbox a',
      definition: 'Two recorded events.',
      coverage: 'unknown',
      checkedAt: undefined,
      surface: 'inbox',
      metric: 'a',
      value: 4,
      unit: 'count',
      source: 'conversation-core',
      eventCount: 2,
      aggregation: 'sum_of_recorded_events',
    })
    // Unattributed scorecard keeps an empty source — never fabricated.
    expect(overview.scorecards[1]!.source).toBe('')
  })
})

describe('normalizeInsightConnector', () => {
  it('accepts the owner contract and the legacy presentation shape during the rollout', () => {
    expect(normalizeInsightConnector({
      type: 'conversation-core',
      surface: 'inbox',
      display_name: 'Verevon Inbox',
      status: 'native',
    })).toEqual({ id: 'conversation-core', kind: 'inbox', label: 'Verevon Inbox', status: 'native' })
    expect(normalizeInsightConnector({
      id: 'legacy',
      kind: 'knowledge',
      label: 'Legacy connector',
      status: 'planned',
    })).toEqual({ id: 'legacy', kind: 'knowledge', label: 'Legacy connector', status: 'planned' })
  })
})

describe('normalizeInsightOverview', () => {
  it('rejects malformed reports instead of turning them into empty measurements', () => {
    expect(() => normalizeInsightOverview(null)).toThrow('Invalid Insight Core report')
    expect(() => normalizeInsightOverview({})).toThrow('Incomplete Insight Core report')
  })

  it('preserves report metadata and rejects malformed numeric scorecards', () => {
    const overview = normalizeInsightOverview({
      contract_version: '2.0',
      coverage: 'unknown',
      coverage_detail: 'No delivery watermark.',
      generated_at: '2026-09-30T12:00:00Z',
      window: { from: '2026-09-01T00:00:00Z', to: '2026-09-30T12:00:00Z' },
      scorecards: [{ id: 'x.y', label: 'Metric Y', metric: 'y', surface: 'x', value: 2, unit: 'count', source: 'core', event_count: 2, aggregation: 'sum_of_recorded_events' }],
    })
    expect(overview.sourceCount).toBe(1)
    expect(overview.coverage).toBe('unknown')
    expect(overview.contractVersion).toBe('2.0')
    expect(overview.coverageDetail).toBe('No delivery watermark.')
    expect(overview.window.from).toBe('2026-09-01T00:00:00Z')
    expect(() => normalizeInsightOverview({
      generated_at: '2026-09-30T12:00:00Z',
      scorecards: [{ id: 'x.y', label: 'Metric Y', metric: 'y', surface: 'x', value: 'NaN', unit: 'count', event_count: 2, aggregation: 'sum_of_recorded_events' }],
    })).toThrow('Invalid numeric value in Insight Core report')
  })
})

function jsonResponse(body: unknown, status = 200) {
  return new Response(JSON.stringify({ data: body }), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}
