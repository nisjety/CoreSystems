import { beforeEach, describe, expect, it, vi } from 'vitest'
vi.mock('./http', () => ({ requestJson: vi.fn() }))
import { requestJson } from './http'
import { loadAquatiqUpdates } from './home-updates-client'

const page = (id: string, metadata = {}, extra = {}) => ({ document_id: id, source: 'sharepoint', type: 'sharepoint_page', title: id,
  metadata: { web_url: `https://aquatiq.sharepoint.com/sites/news/SitePages/${id}.aspx`, site_web_url: 'https://aquatiq.sharepoint.com/sites/news', ...metadata }, ...extra })
beforeEach(() => vi.mocked(requestJson).mockReset())
describe('Aquatiq synced page feed', () => {
  it('uses the scoped documents contract, retains source dates, and exposes a bounded result', async () => {
    vi.mocked(requestJson).mockResolvedValue({ documents: [page('old', {}, { document_date: '2026-09-01' }), page('new', {}, { document_date: '2026-09-28' })], total: 120 })
    const controller = new AbortController()
    const result = await loadAquatiqUpdates('org-a', controller.signal)
    expect(requestJson).toHaveBeenCalledWith('/api/v1/knowledge/documents?type=sharepoint_page&limit=100&offset=0', { headers: { 'X-Org-Id': 'org-a' }, signal: controller.signal })
    expect(result.pages.map((item) => item.id)).toEqual(['new', 'old'])
    expect(result.pages[0]?.updatedAt).toBe('2026-09-28')
    expect(result.hasMore).toBe(true)
  })
  it('excludes home pages, known drafts, other sources, and unsafe source links', async () => {
    vi.mocked(requestJson).mockResolvedValue({ documents: [page('good'), page('home', { page_layout: 'home' }), page('draft', { publishing_level: 'draft' }),
      page('wrong-source', {}, { source: 'web' }), page('script', { web_url: 'javascript:alert(1)' }), page('spoof', { web_url: 'https://sharepoint.com.evil.test/a' })] })
    expect((await loadAquatiqUpdates('org-a')).pages.map((item) => item.id)).toEqual(['good'])
  })
  it('distinguishes malformed or unavailable responses from an empty feed', async () => {
    vi.mocked(requestJson).mockResolvedValueOnce({ documents: [] }).mockResolvedValueOnce({ documents: null, total: 0 }).mockResolvedValueOnce({})
    expect((await loadAquatiqUpdates('org-a')).pages).toEqual([])
    expect((await loadAquatiqUpdates('org-a')).pages).toEqual([])
    await expect(loadAquatiqUpdates('org-a')).rejects.toThrow('unavailable')
  })
})
