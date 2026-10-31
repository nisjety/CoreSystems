import { beforeEach, afterEach, expect, it, vi } from 'vitest'
import { loadTenderMatchingContext, professionalKeywords } from './anbud-matching-context'
const sources = vi.hoisted(() => ({ me: vi.fn(), websites: vi.fn(), search: vi.fn(), org: 'org-a', user: 'user-a' }))
vi.mock('./settings-client', () => ({ getMe: sources.me }))
vi.mock('./marketing/website-client', () => ({ loadWebsites: sources.websites }))
vi.mock('./marketing/search-client', () => ({ loadSearch: sources.search }))
vi.mock('@/shared/session/session-store', () => ({ getSession: () => ({ activeOrg: { id: sources.org }, user: { id: sources.user } }) }))
beforeEach(() => {
  sources.org = 'org-a'; sources.user = 'user-a'
  sources.me.mockReset().mockResolvedValue({ position: 'Engineering Manager', department: 'IT', email: 'private@example.com', phoneNumber: 'private', name: 'Private person' })
  sources.websites.mockReset().mockResolvedValue([{ id: 'primary', site: 'https://example.com/', primary: true, ownership: { status: 'google_owner' }, integrations: { searchConsole: { status: 'connected' } } }])
  sources.search.mockReset().mockResolvedValue({ status: 'connected', updatedAt: new Date().toISOString(), queries: [{ key: 'cybersecurity services', clicks: 5, impressions: 10 }] })
})
afterEach(() => vi.restoreAllMocks())
it('uses only professional fields and current own-site search terms', async () => {
  expect(await loadTenderMatchingContext()).toEqual({ companyKeywords: ['cybersecurity services'], userKeywords: ['it', 'engineering'] })
  expect(sources.search).toHaveBeenCalledWith('FY', 'https://example.com/', expect.any(AbortSignal), 'primary')
})
it('ignores generic role words and matches IT as a bounded work term', () => {
  expect(professionalKeywords('CEO and Founder', 'IT')).toEqual(['it'])
})
it('ignores stale SEO and unverified competitor sites', async () => {
  sources.search.mockResolvedValue({ status: 'connected', updatedAt: '2020-01-01T00:00:00Z', queries: [{ key: 'stale', impressions: 10, clicks: 2 }] })
  expect((await loadTenderMatchingContext()).companyKeywords).toEqual([])
  sources.websites.mockResolvedValue([{ primary: false, ownership: { status: 'google_owner' }, integrations: { searchConsole: { status: 'connected' } } }])
  sources.search.mockClear()
  expect((await loadTenderMatchingContext()).companyKeywords).toEqual([])
  expect(sources.search).not.toHaveBeenCalled()
})
it('discards context if the active organisation or user changes during loading', async () => {
  sources.me.mockImplementation(async () => { sources.user = 'user-b'; return { position: 'Security', department: 'IT' } })
  expect(await loadTenderMatchingContext()).toEqual({ companyKeywords: [], userKeywords: [] })
})
it('degrades to company-profile matching when context services are unavailable', async () => {
  sources.me.mockRejectedValue(new Error('unavailable')); sources.websites.mockRejectedValue(new Error('unavailable'))
  expect(await loadTenderMatchingContext()).toEqual({ companyKeywords: [], userKeywords: [] })
})
