import { afterEach, describe, expect, it, vi } from 'vitest'
import { isSearchSnapshot, loadSearch, sanitizeSearchSnapshot } from './search-client.ts'

const row = { key: 'water hygiene', clicks: 2, impressions: 200, ctr: 0.01, position: 5 }
function snapshot() {
  return {
    site: 'https://aquatiq.com/', source: 'google_search_console', status: 'connected',
    period: { startDate: '2026-10-01', endDate: '2026-10-20' },
    queries: [{ ...row }], pages: [{ ...row, key: 'https://aquatiq.com/' }],
    opportunities: [{ ...row, type: 'title_meta' }], rowLimit: 100,
    queriesAtLimit: false, pagesAtLimit: false, propertyScope: 'domain', updatedAt: '2026-10-21T12:00:00Z',
  }
}

describe('Search intelligence projection', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('allowlists the exact nested projection and never forwards unknown credential fields', () => {
    const input = { ...snapshot(), token: 'private', period: { ...snapshot().period, secret: 'private' },
      queries: [{ ...row, accessToken: 'private' }], opportunities: [{ ...row, type: 'title_meta', provider: 'private' }] }
    const result = sanitizeSearchSnapshot(input)
    expect(result).toEqual(snapshot())
    expect(JSON.stringify(result)).not.toContain('private')
    expect(result.queries[0]).not.toBe(input.queries[0])
  })

  it('accepts bounded real rows, zero metrics, and honest unavailable snapshots', () => {
    expect(isSearchSnapshot(snapshot())).toBe(true)
    expect(isSearchSnapshot({ ...snapshot(), queries: [{ key: 'zero', clicks: 0, impressions: 0, ctr: 0, position: 0 }], opportunities: [] })).toBe(true)
    expect(isSearchSnapshot({ ...snapshot(), status: 'unavailable', reason: 'property_unverified', queries: [], pages: [], opportunities: [] })).toBe(true)
    expect(isSearchSnapshot({ ...snapshot(), status: 'unavailable' })).toBe(false)
  })

  it.each([
    { clicks: -1 }, { clicks: 1.2 }, { clicks: Number.MAX_SAFE_INTEGER + 1 },
    { clicks: 201 }, { impressions: Infinity }, { ctr: 1.1 }, { ctr: 0.2 },
    { position: -1 }, { position: NaN }, { position: 0 }, { position: 0.5 }, { key: '' }, { key: 'bad\nquery' }, { key: 'ø'.repeat(257) },
  ])('rejects unsafe or incoherent metrics %j', (patch) => {
    expect(isSearchSnapshot({ ...snapshot(), queries: [{ ...row, ...patch }], opportunities: [] })).toBe(false)
  })

  it('rejects duplicate keys, excess rows, oversized pages and invalid metadata', () => {
    expect(isSearchSnapshot({ ...snapshot(), queries: [row, row] })).toBe(false)
    expect(isSearchSnapshot({ ...snapshot(), pages: Array.from({ length: 101 }, (_, i) => ({ ...row, key: `${i}` })) })).toBe(false)
    expect(isSearchSnapshot({ ...snapshot(), pages: [{ ...row, key: 'a'.repeat(2049) }] })).toBe(false)
    for (const patch of [{ site: 'https://aquatiq.com/path' }, { rowLimit: 101 }, { queriesAtLimit: 'false' },
      { queriesAtLimit: true }, { propertyScope: 'unknown' }, { reason: 'secret' }, { reason: ['not_configured'] }, { reason: 'not_configured' }, { updatedAt: 'invalid' },
      { period: { startDate: '2026-02-30', endDate: '2026-03-01' } }]) {
      expect(isSearchSnapshot({ ...snapshot(), ...patch })).toBe(false)
    }
  })

  it('only accepts deterministic opportunities backed by the exact returned query', () => {
    for (const patch of [{ key: 'unknown' }, { clicks: 1, ctr: 0.005 }, { type: 'snippet_review' }, { type: 'guaranteed_growth' }]) {
      expect(isSearchSnapshot({ ...snapshot(), opportunities: [{ ...row, type: 'title_meta', ...patch }] })).toBe(false)
    }
    const cases = [
      { position: 3, clicks: 2, ctr: 0.01, type: 'snippet_review' },
      { position: 3.1, clicks: 2, ctr: 0.01, type: 'title_meta' },
      { position: 10, clicks: 6, ctr: 0.03, type: 'title_test' },
      { position: 20, clicks: 8, ctr: 0.04, type: 'content_expansion' },
    ]
    for (const { type, ...patch } of cases) {
      const query = { ...row, ...patch }
      expect(isSearchSnapshot({ ...snapshot(), queries: [query], opportunities: [{ ...query, type }] })).toBe(true)
    }
    expect(isSearchSnapshot({ ...snapshot(), opportunities: [{ ...row, type: 'title_meta' }, { ...row, type: 'title_meta' }] })).toBe(false)
    expect(() => sanitizeSearchSnapshot(null)).toThrow('Invalid Search response')
  })

  it('rejects malformed structures, thresholds and invalid opportunity positions', () => {
    for (const value of [null, [], 1, { ...snapshot(), site: 'https://127.0.0.1/' }, { ...snapshot(), site: 1 },
      { ...snapshot(), source: 'other' }, { ...snapshot(), status: 'invented' }, { ...snapshot(), period: null },
      { ...snapshot(), period: { startDate: 1, endDate: '2026-10-20' } }, { ...snapshot(), queries: null },
      { ...snapshot(), pages: [null] }, { ...snapshot(), opportunities: [{}] }]) expect(isSearchSnapshot(value)).toBe(false)
    for (const patch of [{ impressions: 50, clicks: 1, ctr: 0.02 }, { clicks: 10, ctr: 0.05 }, { position: 20.1 }, { position: 2, clicks: 8, ctr: 0.04 }]) {
      const query = { ...row, ...patch }
      expect(isSearchSnapshot({ ...snapshot(), queries: [query], opportunities: [{ ...query, type: 'snippet_review' }] })).toBe(false)
    }
    const minimal = { ...snapshot(), propertyScope: undefined, updatedAt: undefined, reason: undefined }
    expect(sanitizeSearchSnapshot(minimal)).not.toHaveProperty('updatedAt')
    expect(sanitizeSearchSnapshot({ ...minimal, status: 'unavailable', queries: [], pages: [], opportunities: [], reason: 'source_unavailable' })).toHaveProperty('reason', 'source_unavailable')
  })

  it('loads the same-origin read-only projection without browser credentials', async () => {
    const fetchMock = vi.fn().mockResolvedValue(new Response(JSON.stringify({ data: snapshot() })))
    vi.stubGlobal('fetch', fetchMock)
    const controller = new AbortController()
    expect(await loadSearch('Q4', 'https://aquatiq.com/', controller.signal)).toEqual(snapshot())
    expect(fetchMock).toHaveBeenCalledWith('/api/v1/visibility/search?quarter=Q4&site=https%3A%2F%2Faquatiq.com%2F', {
      credentials: 'include', headers: { Accept: 'application/json' }, signal: controller.signal,
    })
  })

  it('rejects mismatched sites, bad envelopes, invalid sites and unavailable APIs', async () => {
    const fetchMock = vi.fn().mockResolvedValue(new Response(JSON.stringify({ data: snapshot() })))
    vi.stubGlobal('fetch', fetchMock)
    await expect(loadSearch('Q4', 'https://example.com/')).rejects.toThrow('Invalid Search response')
    await expect(loadSearch('Q4', 'https://aquatiq.com/path')).rejects.toThrow('Invalid Search site')
    fetchMock.mockResolvedValue(new Response('{}'))
    await expect(loadSearch('Q4', 'https://aquatiq.com/')).rejects.toThrow('Invalid Search response')
    fetchMock.mockResolvedValue(new Response('{}', { status: 401 }))
    await expect(loadSearch('Q4', 'https://aquatiq.com/')).rejects.toThrow('Search data is unavailable')
  })
})
