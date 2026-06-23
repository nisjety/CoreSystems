import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  getInsightsOverview,
  listInsightConnectors,
  normalizeInsightOverview,
  type InsightConnector,
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
    expect(headers.has('x-velion-org-id')).toBe(false)
    expect(headers.has('x-internal-api-key')).toBe(false)
    expect(headers.has('x-api-key')).toBe(false)
    // Same-origin, cookie-authenticated request.
    expect(init?.credentials).toBe('include')
  })

  it('parses the gateway { data: [...] } envelope into connectors', async () => {
    const connectors: InsightConnector[] = [
      { id: 'social-core', kind: 'social', label: 'Velion Social', status: 'native' },
      { id: 'conversation-core', kind: 'inbox', label: 'Velion Inbox', status: 'native' },
      { id: 'model-plane-agents', kind: 'agents', label: 'Velion Agents', status: 'planned' },
      { id: 'google_analytics_4', kind: 'external_analytics', label: 'Google Analytics 4', status: 'not_connected' },
    ]
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse(connectors)))

    const result = await listInsightConnectors()

    expect(result).toEqual(connectors)
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
    expect(headers.has('x-velion-org-id')).toBe(false)
    expect(headers.has('x-internal-api-key')).toBe(false)
    expect(init?.credentials).toBe('include')
  })

  it('normalizes the gateway overview into real camelCase scorecards', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        jsonResponse({
          generated_at: '2026-06-20T10:00:00Z',
          source_count: 2,
          scorecards: [
            { id: 'inbox.a', label: 'inbox a', surface: 'inbox', metric: 'a', value: 4, unit: 'count', source: 'conversation-core' },
            { id: 'social.b', label: 'social b', surface: 'social', metric: 'b', value: 2.5, unit: '', source: '' },
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
      surface: 'inbox',
      metric: 'a',
      value: 4,
      unit: 'count',
      source: 'conversation-core',
    })
    // Unattributed scorecard keeps an empty source — never fabricated.
    expect(overview.scorecards[1]!.source).toBe('')
  })
})

describe('normalizeInsightOverview', () => {
  it('collapses a non-object / empty payload to a zero-scorecard overview', () => {
    expect(normalizeInsightOverview(null)).toEqual({ generatedAt: '', scorecards: [], sourceCount: 0 })
    expect(normalizeInsightOverview({})).toEqual({ generatedAt: '', scorecards: [], sourceCount: 0 })
  })

  it('never fabricates a value or source for malformed scorecard fields', () => {
    const overview = normalizeInsightOverview({
      scorecards: [{ id: 'x.y', metric: 'y', surface: 'x', value: 'NaN', source: 42 }],
    })
    expect(overview.scorecards[0]).toEqual({
      id: 'x.y',
      label: '',
      surface: 'x',
      metric: 'y',
      value: 0,
      unit: '',
      source: '',
    })
    // source_count falls back to the real produced row count when absent.
    expect(overview.sourceCount).toBe(1)
  })
})

function jsonResponse(body: unknown, status = 200) {
  return new Response(JSON.stringify({ data: body }), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}
