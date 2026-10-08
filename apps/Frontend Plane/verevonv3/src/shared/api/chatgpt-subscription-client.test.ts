import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  disconnectChatGptSubscription,
  getChatGptSubscriptionUsage,
  getChatGptSubscriptionStatus,
  isChatGptSubscriptionConnected,
  listChatGptSubscriptions,
  startChatGptSubscription,
} from './chatgpt-subscription-client'

afterEach(() => vi.unstubAllGlobals())

describe('ChatGPT subscription client', () => {
  it('starts the device-code flow through the organization-scoped gateway', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({ data: {
      connection: { id: 'conn-codex', providerKey: 'openai-codex-subscription', status: 'pending' },
      login: {
        loginId: 'login-codex',
        connectionId: 'conn-codex',
        verificationUrl: 'https://auth.openai.com/codex/device',
        userCode: 'ABCD-EFGH',
      },
    } }), { headers: { 'Content-Type': 'application/json' } })))

    await expect(startChatGptSubscription('org-coresystem')).resolves.toEqual(expect.objectContaining({
      connection: expect.objectContaining({ id: 'conn-codex' }),
      login: expect.objectContaining({ userCode: 'ABCD-EFGH' }),
    }))

    const request = vi.mocked(fetch).mock.calls[0]
    expect(String(request?.[0])).toBe('/api/v1/model-subscriptions/openai-codex/connect')
    expect(request?.[1]?.method).toBe('POST')
    expect(request?.[1]?.body).toBe('{}')
    expect(new Headers(request?.[1]?.headers).get('x-verevon-org-id')).toBe('org-coresystem')
  })

  it('polls and disconnects only through scoped, opaque connection routes', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({ data: {
      connection: { id: 'conn-codex', providerKey: 'openai-codex-subscription', status: 'active' },
      login: { loginId: 'login-codex', connectionId: 'conn-codex', verificationUrl: '', userCode: '', status: 'connected' },
    } }), { headers: { 'Content-Type': 'application/json' } })))

    await getChatGptSubscriptionStatus('org-coresystem', 'conn/codex', 'login/codex')
    await disconnectChatGptSubscription('org-coresystem', 'conn/codex')

    const poll = vi.mocked(fetch).mock.calls[0]
    const disconnect = vi.mocked(fetch).mock.calls[1]
    expect(String(poll?.[0])).toBe('/api/v1/model-subscriptions/openai-codex/connect/conn%2Fcodex/login%2Fcodex')
    expect(String(disconnect?.[0])).toBe('/api/v1/model-subscriptions/openai-codex/connections/conn%2Fcodex')
    expect(disconnect?.[1]?.method).toBe('DELETE')
  })

  it('reads provider allowance windows without creating an inference request', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({ data: {
      connectionId: 'conn-codex', scope: 'connected_user', source: 'provider_rate_limits',
      usage: { observedAt: '2026-10-01T10:00:00Z', primary: { usedPercent: 42, windowMinutes: 300 } },
    } }), { headers: { 'Content-Type': 'application/json' } })))

    await expect(getChatGptSubscriptionUsage('org-coresystem', 'conn/codex')).resolves.toMatchObject({
      scope: 'connected_user', source: 'provider_rate_limits', usage: { primary: { usedPercent: 42 } },
    })
    const request = vi.mocked(fetch).mock.calls[0]
    expect(String(request?.[0])).toBe('/api/v1/model-subscriptions/openai-codex/connections/conn%2Fcodex/usage')
    expect(request?.[1]?.method).toBeUndefined()
    expect(new Headers(request?.[1]?.headers).get('x-verevon-org-id')).toBe('org-coresystem')
  })

  it('keeps subscription connections out of generic source rows', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({ data: {
      connections: [
        { id: 'conn-codex', providerKey: 'openai-codex-subscription', status: 'active', createdAt: '2026-09-06T12:00:00Z' },
        { id: 'conn-slack', providerKey: 'slack', status: 'active', createdAt: '2026-09-06T12:00:00Z' },
      ],
    } }), { headers: { 'Content-Type': 'application/json' } })))

    await expect(listChatGptSubscriptions('org-coresystem')).resolves.toEqual([
      expect.objectContaining({ id: 'conn-codex', providerKey: 'openai-codex-subscription' }),
    ])
  })

  it('recognizes both connected status values exposed by Integration Core', () => {
    expect(isChatGptSubscriptionConnected('active')).toBe(true)
    expect(isChatGptSubscriptionConnected('connected')).toBe(true)
    expect(isChatGptSubscriptionConnected('needs_refresh')).toBe(false)
  })
})
