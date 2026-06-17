import { afterEach, describe, expect, it, vi } from 'vitest'
import { getInsightsOverview, listInsightConnectors } from '@/shared/api/insights-client'

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('insights-client', () => {
  it('uses gateway-facing Insights routes without internal service keys', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse({ connectors: [] })))

    await listInsightConnectors('org_acme')

    const [url, init] = vi.mocked(fetch).mock.calls[0]!
    const headers = init?.headers as Headers

    expect(String(url)).toBe('/api/v1/insights/connectors')
    expect(headers.get('x-velion-org-id')).toBe('org_acme')
    expect(headers.has('x-internal-api-key')).toBe(false)
    expect(headers.has('x-api-key')).toBe(false)
  })

  it('reads the overview through the same gateway path', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse({ status: 'ok', metrics: [] })))

    await getInsightsOverview('org_acme')

    const [url, init] = vi.mocked(fetch).mock.calls[0]!
    const headers = init?.headers as Headers

    expect(String(url)).toBe('/api/v1/insights/overview')
    expect(headers.get('x-velion-org-id')).toBe('org_acme')
    expect(headers.has('x-internal-api-key')).toBe(false)
  })
})

function jsonResponse(body: unknown, status = 200) {
  return new Response(JSON.stringify({ data: body }), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}
