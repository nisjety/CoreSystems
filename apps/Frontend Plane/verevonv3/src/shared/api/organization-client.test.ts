import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  getOrganizationAISettings,
  getOrganizationZdr,
  getOrganizationSupportAIMode,
  getOrganizationZdrEntitled,
  listOrganizations,
  switchActiveOrganization,
  updateOrganizationZdr,
  updateOrganizationSupportAIMode,
} from './organization-client'

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}

describe('organization client', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('lists canonical Auth organizations without treating metadata as billing authority', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse([
      { id: 'org_1', name: 'Acme', slug: 'acme', metadata: '{"plan":"enterprise"}' },
      { id: 'org_2', name: 'Beta', slug: 'beta', metadata: { plan: 'trial' } },
    ])))

    await expect(listOrganizations()).resolves.toEqual([
      { id: 'org_1', name: 'Acme', slug: 'acme' },
      { id: 'org_2', name: 'Beta', slug: 'beta' },
    ])
  })

  it('switches active organization through the session-only gateway contract', async () => {
    const fetchMock = vi.fn(async () => jsonResponse({ organization: { id: 'org_2' } }))
    vi.stubGlobal('fetch', fetchMock)

    await switchActiveOrganization('org_2')

    const [path, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit]
    expect(path).toBe('/api/v1/orgs/switch-active')
    expect(init.method).toBe('POST')
    expect(JSON.parse(String(init.body))).toEqual({ organizationId: 'org_2' })
  })

  it('reads the stored Zero Data Retention posture from org metadata', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        jsonResponse({ id: 'org_1', name: 'Acme', metadata: { interactiveRetention: { zdr: false } } }),
      ),
    )

    const zdr = await getOrganizationZdr('org_1')

    const [path] = vi.mocked(fetch).mock.calls[0] as unknown as [string, RequestInit]
    expect(path).toBe('/api/v1/orgs/org_1')
    expect(zdr).toBe(false)
  })

  it('defaults to ZDR-off (product default) when no posture is stored', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse({ id: 'org_1', name: 'Acme', metadata: {} })))
    await expect(getOrganizationZdr('org_1')).resolves.toBe(false)
  })

  it('persists ZDR through the org-admin settings route with a boolean body', async () => {
    const fetchMock = vi.fn(async () =>
      jsonResponse({ id: 'org_1', name: 'Acme', metadata: { interactiveRetention: { zdr: false } } }),
    )
    vi.stubGlobal('fetch', fetchMock)

    const result = await updateOrganizationZdr('org_1', false)

    const [path, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit]
    expect(path).toBe('/api/v1/orgs/org_1/settings')
    expect(init.method).toBe('PATCH')
    expect(JSON.parse(String(init.body))).toEqual({ zeroDataRetention: false })
    expect(result).toBe(false)
  })

  it('reads and persists the bounded support AI mode through the org settings route', async () => {
    const fetchMock = vi.fn(async () => jsonResponse({ id: 'org_1', metadata: { supportAi: { mode: 'assist' } } }))
    vi.stubGlobal('fetch', fetchMock)
    await expect(getOrganizationSupportAIMode('org_1')).resolves.toBe('assist')
    await expect(updateOrganizationSupportAIMode('org_1', 'assist')).resolves.toBe('assist')
    const [, init] = fetchMock.mock.calls[1] as unknown as [string, RequestInit]
    expect(JSON.parse(String(init.body))).toEqual({ supportAiMode: 'assist' })
  })

  it('reads the combined AI operating settings in one canonical org request', async () => {
    const fetchMock = vi.fn(async () => jsonResponse({
      id: 'org_1',
      metadata: { interactiveRetention: { zdr: true }, supportAi: { mode: 'assist' } },
    }))
    vi.stubGlobal('fetch', fetchMock)

    await expect(getOrganizationAISettings('org_1')).resolves.toEqual({ zdr: true, supportAiMode: 'assist' })
    expect(fetchMock).toHaveBeenCalledTimes(1)
  })

  it('reports ZDR entitlement from the plan-gated entitlements list', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        jsonResponse({
          organization_id: 'org_1',
          entitlements: [
            { key: 'feature.chat', enabled: true },
            { key: 'feature.zero_data_retention', enabled: true },
          ],
        }),
      ),
    )

    const entitled = await getOrganizationZdrEntitled('org_1')

    const [path] = vi.mocked(fetch).mock.calls[0] as unknown as [string, RequestInit]
    expect(path).toBe('/api/v1/orgs/org_1/entitlements')
    expect(entitled).toBe(true)
  })

  it('treats a disabled or absent ZDR entitlement as not entitled', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        jsonResponse({
          organization_id: 'org_1',
          entitlements: [
            { key: 'feature.chat', enabled: true },
            { key: 'feature.zero_data_retention', enabled: false },
          ],
        }),
      ),
    )

    await expect(getOrganizationZdrEntitled('org_1')).resolves.toBe(false)
  })
})
