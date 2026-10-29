import { afterEach, describe, expect, it, vi } from 'vitest'
import { checkUrlReputation, investigateUrl } from './security-client'

function jsonResponse(data: unknown): Response {
  return new Response(JSON.stringify({ data }), {
    status: 200,
    headers: { 'Content-Type': 'application/json' },
  })
}

describe('security API client', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('posts URL reputation checks through the gateway', async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse({
      id: 'rep_1',
      checkedAt: '2026-06-14T12:00:00Z',
      verdict: 'safe',
      provider: 'google_web_risk',
      matches: [],
      policy: {
        externalLookupUsed: true,
        nextAction: 'allow',
      },
    }))
    vi.stubGlobal('fetch', fetchMock)

    const result = await checkUrlReputation('org_1', {
      url: 'https://example.com/docs',
      allowExternalLookup: true,
      purpose: 'ingestion_guard',
    })

    expect(result.verdict).toBe('safe')
    const [path, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(path).toBe('/api/v1/security/url-reputation-checks')
    expect(init.credentials).toBe('include')
    expect((init.headers as Headers).get('x-verevon-org-id')).toBe('org_1')
    expect(JSON.parse(init.body as string)).toMatchObject({
      url: 'https://example.com/docs',
      allowExternalLookup: true,
      purpose: 'ingestion_guard',
    })
  })

  it('posts urlscan investigations with explicit external submission', async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse({
      id: 'scan_1',
      provider: 'urlscan_io',
      status: 'queued',
      visibility: 'private',
      submittedAt: '2026-06-14T12:00:00Z',
    }))
    vi.stubGlobal('fetch', fetchMock)

    const result = await investigateUrl('org_1', {
      url: 'https://example.com/suspicious',
      allowExternalSubmission: true,
      dataClass: 'public',
      visibility: 'private',
    })

    expect(result.provider).toBe('urlscan_io')
    const [path, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(path).toBe('/api/v1/security/url-investigations')
    expect((init.headers as Headers).get('x-verevon-org-id')).toBe('org_1')
    expect(JSON.parse(init.body as string)).toMatchObject({
      url: 'https://example.com/suspicious',
      allowExternalSubmission: true,
      visibility: 'private',
    })
  })
})
