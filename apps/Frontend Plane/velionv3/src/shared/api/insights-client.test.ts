import { afterEach, describe, expect, it, vi } from 'vitest'
import { listInsightConnectors, type InsightConnector } from '@/shared/api/insights-client'

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
})

function jsonResponse(body: unknown, status = 200) {
  return new Response(JSON.stringify({ data: body }), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}
