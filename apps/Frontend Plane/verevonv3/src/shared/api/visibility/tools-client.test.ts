import { afterEach, describe, expect, it, vi } from 'vitest'
import { analysis, comparisons, context } from './__fixtures__/tools'
import { compareCompetitor, loadAnalysis, loadCompetitors, sanitizeAnalysis, sanitizeCompetitors } from './tools-client'

afterEach(() => vi.unstubAllGlobals())
describe('Visibility analysis boundaries', () => {
  it('preserves unknown versus measured zero and strips nested provider fields', () => {
    const value = analysis()
    const result = sanitizeAnalysis({ ...value, accessToken: 'secret', pages: value.pages.map(p => ({ ...p, html: 'secret' })), edges: value.edges.map(e => ({ ...e, bearer: 'secret' })) }, context)
    expect(result).toEqual(value)
    expect(result.pages[0]!.clicks).toBe(0)
    expect(result.pages[1]!.clicks).toBeNull()
  })
  it('rejects mixed site/revision, invalid dates, unsafe pages and invalid measurements', () => {
    const value = analysis()
    for (const update of [{ websiteRevision: 8 }, { site: 'https://agenci.no/' }, { generatedAt: '2026-02-30T12:00:00Z' }, { period: { startDate: '2026-02-30', endDate: '2026-10-27' } }, { pages: [{ ...value.pages[0], clicks: Number.MAX_SAFE_INTEGER + 1 }] }, { pages: [{ ...value.pages[0], title: 'unsafe\ntext' }] }, { pages: [{ ...value.pages[0], url: 'https://other.no/' }] }, { pages: [{ ...value.pages[0], url: `${context.site}login` }] }, { groups: [] }]) {
      expect(() => sanitizeAnalysis({ ...value, ...update }, context)).toThrow()
    }
  })
  it('loads saved evidence without authority headers or collection commands', async () => {
    const fetcher = vi.fn<(url: string, init?: RequestInit) => Promise<Response>>(async () => new Response(JSON.stringify({ data: analysis() })))
    vi.stubGlobal('fetch', fetcher)
    await loadAnalysis(context, 'Q4')
    expect(fetcher.mock.calls[0]).toMatchObject([expect.stringContaining('quarter=Q4'), { method: 'GET', credentials: 'include', redirect: 'error', headers: { Accept: 'application/json' } }])
    await expect(loadAnalysis(context, 'all')).rejects.toThrow()
    expect(fetcher).toHaveBeenCalledTimes(1)
  })
})
describe('Public comparison boundaries', () => {
  it('requires the selected website context and bounded targets', () => {
    expect(sanitizeCompetitors({ ...comparisons(), jobId: 'private' }, context)).toEqual(comparisons())
    expect(() => sanitizeCompetitors({ ...comparisons(), websiteRevision: 8 }, context)).toThrow()
    expect(() => sanitizeCompetitors({ ...comparisons(), entries: [{ ...context, target: 'https://competitor.no/', status: 'complete', requestedAt: '2026-10-28T12:00:00Z', nextRunAt: '2026-10-29T12:00:00Z' }] }, context)).toThrow()
  })
  it('separates saved reads from explicit, site-bound comparison commands', async () => {
    const fetcher = vi.fn<(url: string, init?: RequestInit) => Promise<Response>>(async () => new Response(JSON.stringify({ data: comparisons() })))
    vi.stubGlobal('fetch', fetcher)
    await loadCompetitors(context)
    await compareCompetitor(context, 'https://competitor.no/', 'refresh')
    expect(fetcher.mock.calls[0]).toMatchObject([expect.stringContaining('/competitors?'), { method: 'GET', credentials: 'include' }])
    expect(fetcher.mock.calls[1]).toMatchObject(['/api/v1/visibility/competitors', { method: 'POST', credentials: 'include', body: JSON.stringify({ ...context, target: 'https://competitor.no/', operation: 'refresh' }) }])
    for (const target of ['https://localhost/', 'https://competitor.no/private', 'https://user:secret@competitor.no/']) await expect(compareCompetitor(context, target, 'refresh')).rejects.toThrow()
    expect(fetcher).toHaveBeenCalledTimes(2)
  })
})
