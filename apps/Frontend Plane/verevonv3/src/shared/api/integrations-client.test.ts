import { afterEach, describe, expect, it, vi } from 'vitest'
import { extendInboxHistory, listConnections, triggerInboxSync } from './integrations-client'

afterEach(() => vi.unstubAllGlobals())

describe('listConnections', () => {
  it('normalizes integration-core connection envelopes and canonical provider fields', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({
      data: {
        connections: [{
          id: 'conn-meta',
          providerKey: 'meta',
          displayName: 'Ima DaCosta',
          status: 'active',
          capabilities: ['social.inbox.read'],
          scopes: ['pages_messaging'],
          providerContext: { page_id: 'page-1' },
          createdAt: '2026-07-19T10:00:00Z',
        }],
      },
    }), { headers: { 'Content-Type': 'application/json' } })))

    const connections = await listConnections('org-aquatiq')

    expect(connections).toEqual([expect.objectContaining({
      id: 'conn-meta',
      providerId: 'meta',
      providerKey: 'meta',
      providerName: 'meta',
      displayName: 'Ima DaCosta',
      metadata: { page_id: 'page-1' },
    })])
    const request = vi.mocked(fetch).mock.calls[0]
    if (!request) throw new Error('Expected a connections request')
    expect(String(request[0])).toContain('/api/v1/integrations/connections')
    expect(new Headers(request[1]?.headers).get('x-verevon-org-id')).toBe('org-aquatiq')
  })
})

describe('extendInboxHistory', () => {
  it('requests the fixed next history window through the organization-scoped gateway', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({
      data: { history: { channel: 'teams', historyDays: 60, queued: true } },
    }), { headers: { 'Content-Type': 'application/json' } })))

    await expect(extendInboxHistory('org-aquatiq', 'conn-microsoft')).resolves.toEqual({
      channel: 'teams',
      historyDays: 60,
      queued: true,
    })

    const request = vi.mocked(fetch).mock.calls[0]
    if (!request) throw new Error('Expected a history-extension request')
    expect(String(request[0])).toBe('/api/v1/integrations/connections/conn-microsoft/inbox-history')
    expect(request[1]?.method).toBe('POST')
    expect(new Headers(request[1]?.headers).get('x-verevon-org-id')).toBe('org-aquatiq')
  })
})

describe('triggerInboxSync', () => {
  it('queues a provider-backed inbox fetch through the scoped gateway', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({
      data: { syncJob: { id: 'sync-inbox-1', connectionId: 'conn-google', status: 'waiting_provider' } },
    }), { headers: { 'Content-Type': 'application/json' } })))

    await expect(triggerInboxSync('org-aquatiq', 'conn-google', 'email')).resolves.toEqual({
      syncJob: expect.objectContaining({ id: 'sync-inbox-1', connectionId: 'conn-google' }),
    })

    const request = vi.mocked(fetch).mock.calls[0]
    if (!request) throw new Error('Expected an inbox sync request')
    expect(String(request[0])).toBe('/api/v1/integrations/connections/conn-google/inbox-sync')
    expect(request[1]?.method).toBe('POST')
    expect(request[1]?.body).toBe(JSON.stringify({ channel: 'email' }))
    expect(new Headers(request[1]?.headers).get('x-verevon-org-id')).toBe('org-aquatiq')
  })
})
