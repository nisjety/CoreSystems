import { afterEach, describe, expect, it, vi } from 'vitest'
import { getRouterPolicy, updateRouterPolicy, type RoutingPolicy } from './router-policy-client'

function jsonResponse(data: unknown): Response {
  return new Response(JSON.stringify({ data }), {
    status: 200,
    headers: { 'Content-Type': 'application/json' },
  })
}

function samplePolicy(): RoutingPolicy {
  return {
    enabled: true,
    budget_cap_usd: 5,
    constrained_fraction: 0.25,
    cheap_fallback: 'model-router',
    complexity: {
      large_total_chars: 4000,
      large_total_chars_score: 3,
      medium_total_chars: 1200,
      medium_total_chars_score: 1,
      long_user_turn_chars: 800,
      long_user_turn_score: 1,
      code_fence_score: 2,
      keyword_score: 1,
      tool_use_score: 2,
      deep_conversation_turns: 12,
      deep_conversation_score: 1,
      moderate_threshold: 2,
      complex_threshold: 4,
      keywords: ['analyze', 'refactor'],
    },
    table: {
      budget: { simple: 'gpt-4o-mini', moderate: 'gpt-4o', complex: 'gpt-4.1' },
      balance: { simple: 'gpt-4o', moderate: 'gpt-4.1', complex: 'claude-sonnet' },
      genius: { simple: 'gpt-4.1', moderate: 'claude-sonnet', complex: 'claude-opus' },
    },
    version: 7,
    updated_by: 'user_1',
    updated_at: '2026-06-16T10:00:00Z',
  }
}

describe('router policy API client', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('fetches the router policy through the gateway with org context', async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse(samplePolicy()))
    vi.stubGlobal('fetch', fetchMock)

    const result = await getRouterPolicy('org_1')

    expect(result.enabled).toBe(true)
    expect(result.table.balance.complex).toBe('claude-sonnet')
    const [path, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(path).toBe('/api/v1/router-policy')
    expect(init.method).toBeUndefined()
    expect(init.credentials).toBe('include')
    expect((init.headers as Headers).get('x-verevon-org-id')).toBe('org_1')
  })

  it('PUTs the full policy with server-authored fields zeroed and without mutating input', async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse(samplePolicy()))
    vi.stubGlobal('fetch', fetchMock)
    const input = samplePolicy()

    await updateRouterPolicy('org_1', input)

    const [path, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(path).toBe('/api/v1/router-policy')
    expect(init.method).toBe('PUT')
    expect((init.headers as Headers).get('x-verevon-org-id')).toBe('org_1')

    const body = JSON.parse(init.body as string) as RoutingPolicy
    expect(body.enabled).toBe(true)
    expect(body.budget_cap_usd).toBe(5)
    expect(body.table.genius.complex).toBe('claude-opus')
    expect(body.complexity.keywords).toEqual(['analyze', 'refactor'])
    // Server authors these — client zeroes them out.
    expect(body.version).toBe(0)
    expect(body.updated_by).toBe('')
    expect(body.updated_at).toBe('')

    // Caller's object must not be mutated.
    expect(input.version).toBe(7)
    expect(input.updated_by).toBe('user_1')
    expect(input.updated_at).toBe('2026-06-16T10:00:00Z')
  })
})
