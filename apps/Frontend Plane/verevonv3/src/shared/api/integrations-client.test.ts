import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  browseConnection,
  extendInboxHistory,
  fetchConnectionContent,
  listConnections,
  listConnectorPickerTabs,
  triggerInboxSync,
} from './integrations-client'

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

    const connections = await listConnections('org-coresystem')

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
    expect(new Headers(request[1]?.headers).get('x-verevon-org-id')).toBe('org-coresystem')
  })
})

describe('extendInboxHistory', () => {
  it('requests the fixed next history window through the organization-scoped gateway', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({
      data: { history: { channel: 'teams', historyDays: 60, queued: true } },
    }), { headers: { 'Content-Type': 'application/json' } })))

    await expect(extendInboxHistory('org-coresystem', 'conn-microsoft')).resolves.toEqual({
      channel: 'teams',
      historyDays: 60,
      queued: true,
    })

    const request = vi.mocked(fetch).mock.calls[0]
    if (!request) throw new Error('Expected a history-extension request')
    expect(String(request[0])).toBe('/api/v1/integrations/connections/conn-microsoft/inbox-history')
    expect(request[1]?.method).toBe('POST')
    expect(new Headers(request[1]?.headers).get('x-verevon-org-id')).toBe('org-coresystem')
  })
})

describe('listConnectorPickerTabs', () => {
  it('keeps only active Microsoft/Google connections, keyed by connectionId with displayName as the label', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({
      data: {
        connections: [
          { id: 'conn-ms-1', providerKey: 'microsoft', displayName: 'Ima (work)', status: 'active', createdAt: '2026-01-01T00:00:00Z' },
          { id: 'conn-ms-2', providerKey: 'microsoft', displayName: 'Ima (personal)', status: 'active', createdAt: '2026-01-01T00:00:00Z' },
          { id: 'conn-google-1', providerKey: 'google', status: 'active', providerEmail: 'ima@example.com', createdAt: '2026-01-01T00:00:00Z' },
          { id: 'conn-slack-1', providerKey: 'slack', displayName: 'Team', status: 'active', createdAt: '2026-01-01T00:00:00Z' },
          { id: 'conn-ms-3', providerKey: 'microsoft', displayName: 'Disconnected', status: 'disconnected', createdAt: '2026-01-01T00:00:00Z' },
        ],
      },
    }), { headers: { 'Content-Type': 'application/json' } })))

    const tabs = await listConnectorPickerTabs('org-coresystem')

    expect(tabs).toEqual([
      { connectionId: 'conn-ms-1', providerKey: 'microsoft', label: 'Ima (work)' },
      { connectionId: 'conn-ms-2', providerKey: 'microsoft', label: 'Ima (personal)' },
      { connectionId: 'conn-google-1', providerKey: 'google', label: 'ima@example.com' },
    ])
  })
})

describe('browseConnection', () => {
  it('posts the operation and params to the browse route and unwraps the action envelope', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({
      data: { action: { providerKey: 'microsoft', operation: 'drive.files', result: { value: [{ id: 'item-1', name: 'Rapport.docx' }] } } },
    }), { headers: { 'Content-Type': 'application/json' } })))

    const result = await browseConnection('org-coresystem', 'conn-ms-1', 'drive.files', { query: 'rapport' })

    expect(result).toEqual({
      providerKey: 'microsoft',
      operation: 'drive.files',
      result: { value: [{ id: 'item-1', name: 'Rapport.docx' }] },
    })
    const request = vi.mocked(fetch).mock.calls[0]
    if (!request) throw new Error('Expected a browse request')
    expect(String(request[0])).toBe('/api/v1/integrations/connections/conn-ms-1/browse')
    expect(request[1]?.method).toBe('POST')
    expect(request[1]?.body).toBe(JSON.stringify({ operation: 'drive.files', params: { query: 'rapport' } }))
    expect(new Headers(request[1]?.headers).get('x-verevon-org-id')).toBe('org-coresystem')
  })
})

describe('fetchConnectionContent', () => {
  it('posts the operation and params to the content route and unwraps the action envelope', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({
      data: { action: { providerKey: 'microsoft', operation: 'drive.file.content', result: { content_base64: 'AAAA', mime_type: 'application/pdf', size: 4, truncated: false } } },
    }), { headers: { 'Content-Type': 'application/json' } })))

    const result = await fetchConnectionContent('org-coresystem', 'conn-ms-1', 'drive.file.content', { itemId: 'item-1' })

    expect(result.result).toEqual({ content_base64: 'AAAA', mime_type: 'application/pdf', size: 4, truncated: false })
    const request = vi.mocked(fetch).mock.calls[0]
    if (!request) throw new Error('Expected a content request')
    expect(String(request[0])).toBe('/api/v1/integrations/connections/conn-ms-1/content')
    expect(request[1]?.body).toBe(JSON.stringify({ operation: 'drive.file.content', params: { itemId: 'item-1' } }))
  })
})

describe('triggerInboxSync', () => {
  it('queues a provider-backed inbox fetch through the scoped gateway', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({
      data: { syncJob: { id: 'sync-inbox-1', connectionId: 'conn-google', status: 'waiting_provider' } },
    }), { headers: { 'Content-Type': 'application/json' } })))

    await expect(triggerInboxSync('org-coresystem', 'conn-google', 'email')).resolves.toEqual({
      syncJob: expect.objectContaining({ id: 'sync-inbox-1', connectionId: 'conn-google' }),
    })

    const request = vi.mocked(fetch).mock.calls[0]
    if (!request) throw new Error('Expected an inbox sync request')
    expect(String(request[0])).toBe('/api/v1/integrations/connections/conn-google/inbox-sync')
    expect(request[1]?.method).toBe('POST')
    expect(request[1]?.body).toBe(JSON.stringify({ channel: 'email' }))
    expect(new Headers(request[1]?.headers).get('x-verevon-org-id')).toBe('org-coresystem')
  })
})
