import { afterEach, describe, expect, it, vi } from 'vitest'
import { requestJson } from './http'

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}

describe('gateway HTTP client auth headers', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
    vi.unstubAllEnvs()
  })

  it('does not inject the dev auth bypass unless the environment explicitly opts in', async () => {
    // Pin the opt-in OFF so the assertion is deterministic regardless of any
    // ambient local .env (dev shells set VITE_ALLOW_DEV_AUTH_BYPASS=true).
    vi.stubEnv('VITE_ALLOW_DEV_AUTH_BYPASS', 'false')
    const fetchMock = vi.fn(async () => jsonResponse({ data: { ok: true } }))
    vi.stubGlobal('fetch', fetchMock)

    await requestJson('/api/v1/session/current')

    const [, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit]
    expect((init.headers as Headers).get('Authorization')).toBeNull()
  })

  it('injects the dev auth bypass only when the environment explicitly opts in', async () => {
    vi.stubEnv('VITE_ALLOW_DEV_AUTH_BYPASS', 'true')
    const fetchMock = vi.fn(async () => jsonResponse({ data: { ok: true } }))
    vi.stubGlobal('fetch', fetchMock)

    await requestJson('/api/v1/session/current')

    const [, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit]
    expect((init.headers as Headers).get('Authorization')).toBe('Bearer dev-bypass')
  })
})
