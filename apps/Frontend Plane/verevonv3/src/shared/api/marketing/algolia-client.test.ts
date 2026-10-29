import { afterEach, describe, expect, it, vi } from 'vitest'
import { algoliaAuthorizationUrl, loadAlgoliaMapping, saveAlgoliaMapping, disconnectAlgoliaSite, loadAlgoliaConnections, loadAlgoliaApplications, loadAlgoliaIndices, startAlgoliaSession, loadAlgoliaSession, sanitizeAlgoliaMapping } from './algolia-client'
const ctx = { websiteId: `web_${'1'.repeat(32)}`, site: 'https://triodelab.no/', websiteRevision: 7 }
const ids = { connectionId: 'conn_11111111-1111-4111-8111-111111111111', applicationId: 'APP1', indexName: 'public_index' }
const mapping = { ...ctx, status: 'mapped', ...ids }
const session = { id: 'cs_11111111-1111-4111-8111-111111111111', authorizationUrl: 'https://dashboard.algolia.com/2/oauth/authorize?response_type=code&scope=public&resource=https%3A%2F%2Fmcp.algolia.com%2Fmcp&redirect_uri=http%3A%2F%2Flocalhost%3A3026%2Fapi%2Fv1%2Fproviders%2Falgolia%2Fcallback&client_id=public-client&state=opaque-state&code_challenge=abcdefghijklmnopqrstuvwxyz0123456789ABCDE12&code_challenge_method=S256', expiresAt: '2026-10-23T16:00:00Z' }
const response = (data: unknown, status = 200) => new Response(JSON.stringify({ data }), { status })
describe('Algolia guided connection boundary', () => {
  afterEach(() => vi.unstubAllGlobals())
  it('rebuilds scoped mapping and validates every mapping state without exposing private metadata', () => {
    expect(sanitizeAlgoliaMapping({ ...mapping, token: 'secret' }, ctx)).toEqual(mapping)
    for (const status of ['mapped', 'reconnect_required', 'access_check_required']) expect(sanitizeAlgoliaMapping({ ...mapping, status }, ctx).status).toBe(status)
    expect(sanitizeAlgoliaMapping({ ...ctx, status: 'not_mapped' }, ctx)).toEqual({ ...ctx, status: 'not_mapped' })
    for (const change of [{ websiteRevision: 8 }, { site: 'https://other.example/' }, { status: 'not_mapped' }, { connectionId: 'bad/id' }, { applicationId: 'bad id' }, { indexName: 'private index' }, { status: 'active' }]) expect(() => sanitizeAlgoliaMapping({ ...mapping, ...change }, ctx)).toThrow()
  })
  it('accepts only the real read-only Algolia OAuth endpoint, callback, scope, resource and PKCE', () => {
    expect(algoliaAuthorizationUrl(session.authorizationUrl)).toBe(session.authorizationUrl)
    for (const url of [session.authorizationUrl.replace('dashboard.algolia.com', 'evil.example'), session.authorizationUrl.replace('scope=public', 'scope=admin'), session.authorizationUrl.replace('S256', 'plain'), session.authorizationUrl + '&scope=public', session.authorizationUrl.replace('localhost%3A3026', 'evil.example'), session.authorizationUrl + '#private']) expect(() => algoliaAuthorizationUrl(url)).toThrow()
  })
  it('uses exact passive reads and explicit revision-bound mapping writes with no org headers', async () => {
    const fetcher = vi.fn().mockImplementation(() => Promise.resolve(response(mapping))); vi.stubGlobal('fetch', fetcher)
    await loadAlgoliaMapping(ctx); await saveAlgoliaMapping(ctx, { ...ids, apiKey: 'should-not-forward' } as typeof ids)
    fetcher.mockResolvedValue(response({ ...ctx, status: 'not_mapped' })); await disconnectAlgoliaSite(ctx)
    expect(fetcher.mock.calls.map((call) => call[1].method)).toEqual(['GET', 'PUT', 'DELETE'])
    expect(JSON.parse(fetcher.mock.calls[1]![1].body)).toEqual({ ...ctx, ...ids })
    expect(JSON.parse(fetcher.mock.calls[2]![1].body)).toEqual(ctx)
    expect(fetcher.mock.calls.every((call) => !('x-verevon-org-id' in call[1].headers) && call[1].credentials === 'include')).toBe(true)
  })
  it('strictly projects metadata-only grants, application and index choices', async () => {
    const fetcher = vi.fn().mockResolvedValueOnce(response({ connections: [{ id: ids.connectionId, status: 'active', displayName: 'My account', token: 'secret' }] })).mockResolvedValueOnce(response({ applications: [{ id: 'APP1', schema: 'secret' }] })).mockResolvedValueOnce(response({ indices: [{ name: 'public_index', records: ['secret'] }] })); vi.stubGlobal('fetch', fetcher)
    expect(await loadAlgoliaConnections()).toEqual([{ id: ids.connectionId, status: 'active', displayName: 'My account' }])
    expect(await loadAlgoliaApplications(ids.connectionId)).toEqual(['APP1'])
    expect(await loadAlgoliaIndices(ids.connectionId, 'APP1')).toEqual(['public_index'])
    expect(fetcher.mock.calls[2]![0]).toContain('indices?applicationId=APP1')
    fetcher.mockResolvedValue(response({ connections: Array.from({ length: 65 }, () => ({ id: ids.connectionId, status: 'active' })) })); await expect(loadAlgoliaConnections()).rejects.toThrow()
    fetcher.mockResolvedValue(response({ applications: [{ id: 'bad/id' }] })); await expect(loadAlgoliaApplications(ids.connectionId)).rejects.toThrow()
    fetcher.mockResolvedValue(response({ indices: Array.from({ length: 5000 }, (_, i) => ({ name: `index_${i}` })) })); expect(await loadAlgoliaIndices(ids.connectionId, 'APP1')).toHaveLength(5000)
    fetcher.mockResolvedValue(response({ applications: Array.from({ length: 129 }, (_, i) => ({ id: `APP${i}` })) })); await expect(loadAlgoliaApplications(ids.connectionId)).rejects.toThrow()
  })
  it('validates session start/status and refuses unexpected upstream errors', async () => {
    const fetcher = vi.fn().mockResolvedValueOnce(response(session)).mockResolvedValueOnce(response({ id: session.id, status: 'completed', expiresAt: session.expiresAt, token: 'secret' })); vi.stubGlobal('fetch', fetcher)
    expect(await startAlgoliaSession('http://localhost:5173/studio/pulse')).toEqual(session)
    expect(await loadAlgoliaSession(session.id)).toEqual({ id: session.id, status: 'completed', expiresAt: session.expiresAt })
    expect(JSON.parse(fetcher.mock.calls[0]![1].body)).toEqual({ returnUrl: 'http://localhost:5173/studio/pulse' })
    fetcher.mockResolvedValue(new Response('private error', { status: 503 })); await expect(loadAlgoliaConnections()).rejects.toThrow('algolia_unavailable')
    fetcher.mockResolvedValue(response({ ...session, authorizationUrl: 'https://evil.example/' })); await expect(startAlgoliaSession('http://localhost:5173/studio/pulse')).rejects.toThrow()
  })
})
