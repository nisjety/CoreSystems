import { afterEach, describe, expect, it, vi } from 'vitest'
import { addWebsite, loadWebsites, sanitizeWebsite, sanitizeWebsiteList, sanitizeGoogleAuthorization, sanitizeGoogleStatus, saveWebsite, verifyWebsite, connectWebsiteGoogle, websiteGoogleStatus, saveClarityMapping, saveGA4ReportingSite, ga4ReportingSiteOptions, readWebsitePreference, saveWebsitePreference } from './website-client.ts'

export const websiteFixture = {
  id: 'web_' + '1'.repeat(32), site: 'https://aquatiq.com/', revision: 1,
  createdAt: '2026-10-21T00:00:00Z', updatedAt: '2026-10-21T00:00:00Z',
  ownership: { status: 'google_owner', checkedAt: '2026-10-21T00:00:00Z' },
  integrations: {
    searchConsole: { status: 'connected', property: 'sc-domain:aquatiq.com', accessLevel: 'siteOwner' },
    ga4: { status: 'requires_consent' }, clarity: { status: 'not_connected' }, pageSpeed: { status: 'available' },
  },
}

describe('website registry boundary', () => {
  afterEach(() => { vi.unstubAllGlobals(); vi.unstubAllEnvs() })
  it('reconstructs only public mapping fields and ownership evidence', () => {
    expect(sanitizeWebsite({ ...websiteFixture, orgId: 'private', secret: 'private', ownership: { ...websiteFixture.ownership, token: 'private' }, integrations: { ...websiteFixture.integrations, searchConsole: { ...websiteFixture.integrations.searchConsole, connectionId: 'private' } } })).toEqual(websiteFixture)
  })
  it('keeps only an exact paired public GA4 reporting host and drops provider fields', () => {
    const mapped = {
      ...websiteFixture,
      integrations: {
        ...websiteFixture.integrations,
        ga4: { status: 'connected', reportingSite: 'https://www.aquatiq.com/', connectionId: 'private', propertyId: '123', streamId: '456', measurementId: 'G-ABCDE' },
      },
    }
    expect(sanitizeWebsite(mapped)).toEqual({
      ...websiteFixture,
      integrations: { ...websiteFixture.integrations, ga4: { status: 'connected', reportingSite: 'https://www.aquatiq.com/', propertyId: '123', streamId: '456', measurementId: 'G-ABCDE' } },
    })
    expect(ga4ReportingSiteOptions(websiteFixture.site)).toEqual(['https://aquatiq.com/', 'https://www.aquatiq.com/'])
    expect(ga4ReportingSiteOptions('https://www.aquatiq.com/')).toEqual(['https://www.aquatiq.com/', 'https://aquatiq.com/'])
    for (const reportingSite of ['https://analytics.aquatiq.com/', 'https://www.aquatiq.com/path', 'http://www.aquatiq.com/']) {
      expect(() => sanitizeWebsite({ ...mapped, integrations: { ...mapped.integrations, ga4: { ...mapped.integrations.ga4, reportingSite } } })).toThrow()
    }
    expect(sanitizeWebsite({ ...mapped, site: 'https://www.aquatiq.com/', integrations: { ...mapped.integrations, ga4: { ...mapped.integrations.ga4, reportingSite: 'https://aquatiq.com/' } } }).integrations.ga4.reportingSite).toBe('https://aquatiq.com/')
    const temporarilyUnavailable = { ...mapped, integrations: { ...mapped.integrations, ga4: { status: 'source_unavailable', reportingSite: 'https://www.aquatiq.com/' } } }
    expect(sanitizeWebsite(temporarilyUnavailable)).toEqual(temporarilyUnavailable)
    expect(() => sanitizeWebsite({ ...temporarilyUnavailable, integrations: { ...temporarilyUnavailable.integrations, ga4: { ...temporarilyUnavailable.integrations.ga4, reportingSite: 'https://analytics.aquatiq.com/' } } })).toThrow()
    expect(sanitizeWebsite({ ...temporarilyUnavailable, ownership: { status: 'unverified' } }).integrations.ga4.reportingSite).toBe('https://www.aquatiq.com/')
  })
  it('rejects malformed identities, roots, revisions and integration states', () => {
    for (const value of [
      { ...websiteFixture, id: 'web_no' }, { ...websiteFixture, site: 'https://localhost/' },
      { ...websiteFixture, revision: 0 }, { ...websiteFixture, updatedAt: 'tomorrow' },
      { ...websiteFixture, ownership: { status: 'reader_is_owner' } },
      { ...websiteFixture, integrations: { ...websiteFixture.integrations, clarity: { status: 'connected', projectId: 'invalid' } } },
    ]) expect(() => sanitizeWebsite(value)).toThrow()
  })
  it('loads the backend list with session cookies and only explicit normalized commands, never service credentials', async () => {
    const fetcher = vi.fn().mockImplementation(async (_path: string, init?: RequestInit) => new Response(JSON.stringify({ data: !init?.method || init.method === 'GET' ? { websites: [websiteFixture] } : websiteFixture })))
    vi.stubGlobal('fetch', fetcher)
    expect(await loadWebsites()).toEqual([websiteFixture])
    await addWebsite('aquatiq.com')
    await saveWebsite(websiteFixture.id, 'example.com', 1)
    await verifyWebsite(websiteFixture.id)
    expect(fetcher.mock.calls.map((call) => call[1]?.body)).toEqual([undefined, JSON.stringify({ site: 'https://aquatiq.com/' }), JSON.stringify({ site: 'https://example.com/', revision: 1 }), '{}'])
    expect(fetcher.mock.calls.every((call) => call[1]?.credentials === 'include')).toBe(true)
  })
  it('only admits exact Google consent destinations and read-only scopes', () => {
    const value = { sessionId: 'session_123456789', expiresAt: '2026-10-21T00:10:00Z', authorizationUrl: `https://accounts.google.com/o/oauth2/v2/auth?client_id=client&redirect_uri=http%3A%2F%2Flocalhost%3A3026%2Foauth%2Fcallback%2Fgoogle&response_type=code&state=${'s'.repeat(40)}&code_challenge_method=S256&code_challenge=${'c'.repeat(43)}&scope=openid%20email%20https%3A%2F%2Fwww.googleapis.com%2Fauth%2Fanalytics.readonly` }
    expect(sanitizeGoogleAuthorization(value)).toEqual(value)
    for (const url of [value.authorizationUrl.replace('accounts.google.com', 'accounts.google.com.evil.example'), value.authorizationUrl.replace('analytics.readonly', 'analytics.edit'), value.authorizationUrl.replace('localhost%3A3026', 'evil.example')]) expect(() => sanitizeGoogleAuthorization({ ...value, authorizationUrl: url })).toThrow()
  })
  it('uses only the exact deployment-owned production Google callback', () => {
    const callback = 'https://integrations.example.com/oauth/callback/google'
    vi.stubEnv('VITE_MARKETING_GOOGLE_OAUTH_CALLBACK_URL', callback)
    const query = new URLSearchParams({ client_id: 'client', redirect_uri: callback, response_type: 'code', state: 's'.repeat(40), code_challenge_method: 'S256', code_challenge: 'c'.repeat(43), scope: 'openid email https://www.googleapis.com/auth/analytics.readonly' })
    const value = { sessionId: 'session_123', expiresAt: '2026-10-21T00:10:00Z', authorizationUrl: `https://accounts.google.com/o/oauth2/v2/auth?${query}` }
    expect(sanitizeGoogleAuthorization(value)).toEqual(value)
    for (const redirect of ['http://localhost:3026/oauth/callback/google', 'https://foreign.example.com/oauth/callback/google', callback + '?next=evil', callback + '/']) {
      const foreign = new URLSearchParams(query)
      foreign.set('redirect_uri', redirect)
      expect(() => sanitizeGoogleAuthorization({ ...value, authorizationUrl: `https://accounts.google.com/o/oauth2/v2/auth?${foreign}` })).toThrow()
    }
  })
  it('rejects malformed deployment callbacks even when the provider URL matches them', () => {
    for (const callback of [
      '', 'http://localhost:0/oauth/callback/google', 'http://localhost:03026/oauth/callback/google',
      'http://foreign.example.com/oauth/callback/google', 'http://localhost.evil.example:3026/oauth/callback/google',
      'http://0.0.0.0:3026/oauth/callback/google', 'https://user@integrations.example.com/oauth/callback/google',
      'https://integrations.example.com/oauth/callback/google?next=evil', 'https://integrations.example.com/oauth/callback/google?',
      'https://integrations.example.com/oauth/callback/google#', 'https://integrations.example.com/oauth/callback/google#fragment',
      'https://integrations.example.com/oauth/../oauth/callback/google', 'https://integrations.example.com/oauth/%63allback/google',
      'https://integrations.example.com/oauth\\callback/google', 'https://Integrations.example.com/oauth/callback/google',
      'https://integrations.example.com:443/oauth/callback/google', 'https://integrations.example.com/oauth/callback/google ',
      'https://integrations.example.com/another/callback', '/oauth/callback/google', 'javascript:alert(1)',
    ]) {
      vi.stubEnv('VITE_MARKETING_GOOGLE_OAUTH_CALLBACK_URL', callback)
      const query = new URLSearchParams({ client_id: 'client', redirect_uri: callback, response_type: 'code', state: 's'.repeat(40), code_challenge_method: 'S256', code_challenge: 'c'.repeat(43), scope: 'openid email https://www.googleapis.com/auth/analytics.readonly' })
      expect(() => sanitizeGoogleAuthorization({ sessionId: 'session_123', expiresAt: '2026-10-21T00:10:00Z', authorizationUrl: `https://accounts.google.com/o/oauth2/v2/auth?${query}` })).toThrow()
    }
  })
  it('supports explicitly configured loopback callbacks without admitting other local hosts', () => {
    for (const callback of ['http://127.0.0.1:33026/oauth/callback/google', 'http://[::1]:3026/oauth/callback/google']) {
      vi.stubEnv('VITE_MARKETING_GOOGLE_OAUTH_CALLBACK_URL', callback)
      const query = new URLSearchParams({ client_id: 'client', redirect_uri: callback, response_type: 'code', state: 's'.repeat(40), code_challenge_method: 'S256', code_challenge: 'c'.repeat(43), scope: 'openid email https://www.googleapis.com/auth/analytics.readonly' })
      const value = { sessionId: 'session_123', expiresAt: '2026-10-21T00:10:00Z', authorizationUrl: `https://accounts.google.com/o/oauth2/v2/auth?${query}` }
      expect(sanitizeGoogleAuthorization(value)).toEqual(value)
    }
  })
  it('rejects duplicate registry identities and malformed nested status evidence', () => {
    expect(() => sanitizeWebsiteList({ websites: [websiteFixture, websiteFixture] })).toThrow()
    expect(() => sanitizeWebsiteList({ websites: 'sites' })).toThrow()
    expect(() => sanitizeWebsite({ ...websiteFixture, ownership: { status: 'google_owner', checkedAt: 'invalid' } })).toThrow()
    expect(() => sanitizeWebsite({ ...websiteFixture, integrations: { ...websiteFixture.integrations, ga4: { status: 'connected', propertyId: 'G-measurement-is-not-a-property' } } })).toThrow()
    expect(() => sanitizeGoogleStatus({ status: 'auto_created' })).toThrow()
    expect(sanitizeGoogleStatus({ status: 'completed', token: 'private' })).toEqual({ status: 'completed' })
  })
  it('checks Google consent and provider reporting mappings using explicit fixed endpoints', async () => {
    const fetcher = vi.fn().mockResolvedValueOnce(new Response(JSON.stringify({ data: { status: 'completed' } })))
      .mockResolvedValueOnce(new Response(JSON.stringify({ data: { ...websiteFixture, integrations: { ...websiteFixture.integrations, ga4: { status: 'connected', reportingSite: 'https://www.aquatiq.com/' } } } })))
      .mockResolvedValueOnce(new Response(JSON.stringify({ data: websiteFixture })))
    vi.stubGlobal('fetch', fetcher)
    await websiteGoogleStatus(websiteFixture.id, 'session_123')
    await saveClarityMapping(websiteFixture.id, 'z1abticqmb', 1)
    await saveGA4ReportingSite(websiteFixture.id, websiteFixture.site, 'https://www.aquatiq.com/', 1)
    expect(fetcher.mock.calls.map((call) => call[0])).toEqual([
      `/api/v1/visibility/websites/${websiteFixture.id}/google/status?sessionId=session_123`,
      `/api/v1/visibility/websites/${websiteFixture.id}/mappings`,
      `/api/v1/visibility/websites/${websiteFixture.id}/mappings`,
    ])
    expect(fetcher.mock.calls[1]?.[1]?.body).toBe(JSON.stringify({ clarityProjectId: 'z1abticqmb', revision: 1 }))
    expect(fetcher.mock.calls[2]?.[1]?.body).toBe(JSON.stringify({ ga4ReportingSite: 'https://www.aquatiq.com/', revision: 1 }))
  })
  it('fails closed on invalid commands, revision conflicts and untrusted preference IDs', async () => {
    const fetcher = vi.fn().mockResolvedValue(new Response('{}', { status: 409 })); vi.stubGlobal('fetch', fetcher)
    await expect(saveWebsite(websiteFixture.id, websiteFixture.site, 1)).rejects.toThrow(/changed/i)
    await expect(saveWebsite('invalid', websiteFixture.site, 1)).rejects.toThrow()
    await expect(saveWebsite(websiteFixture.id, websiteFixture.site, -1)).rejects.toThrow()
    await expect(websiteGoogleStatus(websiteFixture.id, 'session?spoof')).rejects.toThrow()
    await expect(saveClarityMapping(websiteFixture.id, '../project', 1)).rejects.toThrow()
    await expect(saveGA4ReportingSite(websiteFixture.id, websiteFixture.site, 'https://other.example/', 1)).rejects.toThrow()
    const storage = { getItem: vi.fn().mockReturnValue('org:spoof'), setItem: vi.fn() }; vi.stubGlobal('localStorage', storage)
    expect(readWebsitePreference()).toBeUndefined()
    saveWebsitePreference('invalid'); expect(storage.setItem).not.toHaveBeenCalled()
    saveWebsitePreference(websiteFixture.id); expect(storage.setItem).toHaveBeenCalledWith('studio-pulse-active-website-id-v1', websiteFixture.id)
    storage.getItem.mockReturnValue(websiteFixture.id); expect(readWebsitePreference()).toBe(websiteFixture.id)
    expect(fetcher).toHaveBeenCalledTimes(1)
  })
  it('does not pass a malformed provider authorization response to the UI', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(JSON.stringify({ data: { authorizationUrl: 'https://evil.example' } }))))
    await expect(connectWebsiteGoogle(websiteFixture.id)).rejects.toThrow()
  })
})
