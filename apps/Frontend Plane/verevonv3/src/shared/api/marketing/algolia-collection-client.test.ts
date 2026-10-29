import { afterEach, describe, expect, it, vi } from 'vitest'
import { loadAlgoliaCollection, queueAlgoliaCollection, sanitizeAlgoliaCollection } from './algolia-collection-client'
const context = { websiteId: `web_${'1'.repeat(32)}`, site: 'https://triodelab.no/', websiteRevision: 7 }
const value = { ...context, provider: 'algolia', status: 'queued', nextRunAt: '2026-10-23T12:00:00Z' }
describe('durable Algolia collection receipts', () => {
  afterEach(() => vi.unstubAllGlobals())
  it('validates scoped states, timestamps and safe reasons while dropping upstream secrets', () => {
    expect(sanitizeAlgoliaCollection({ ...value, token: 'drop' }, context)).toEqual(value)
    for (const status of ['idle', 'queued', 'syncing', 'ready', 'needs_attention', 'reconnect_required']) expect(sanitizeAlgoliaCollection({ ...value, status }, context).status).toBe(status)
    for (const change of [{ websiteRevision: 8 }, { provider: 'google' }, { status: 'connected' }, { reason: 'provider secret' }, { lastSuccessAt: 'tomorrow' }]) expect(() => sanitizeAlgoliaCollection({ ...value, ...change }, context)).toThrow()
  })
  it('uses passive GET and explicit202 queue commands with exact site revision and no forged organization', async () => {
    const fetcher = vi.fn().mockImplementation((_url, init) => Promise.resolve(new Response(JSON.stringify({ data: value }), { status: init.method === 'GET' ? 200 : 202 }))); vi.stubGlobal('fetch', fetcher)
    expect(await loadAlgoliaCollection(context)).toEqual(value); expect(await queueAlgoliaCollection(context)).toEqual(value)
    expect(fetcher.mock.calls[0]![0]).toContain('provider=algolia')
    expect(JSON.parse(fetcher.mock.calls[1]![1].body)).toEqual({ ...context, provider: 'algolia' })
    expect(fetcher.mock.calls.every(call => call[1].credentials === 'include' && !('x-org-id' in call[1].headers))).toBe(true)
    fetcher.mockResolvedValue(new Response('secret', { status: 503 })); await expect(loadAlgoliaCollection(context)).rejects.toThrow('algolia_collection_unavailable')
  })
})
