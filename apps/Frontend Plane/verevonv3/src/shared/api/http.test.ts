import { afterEach, describe, expect, it, vi } from 'vitest'
import { ApiError, requestJson, SESSION_EXPIRED_EVENT } from './http'

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

  it('signals session expiry for an unauthorized protected request', async () => {
    const expired = vi.fn()
    window.addEventListener(SESSION_EXPIRED_EVENT, expired)
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse({
      error: { code: 'unauthorized', message: 'Authentication required' },
    }, 401)))

    await expect(requestJson('/api/v1/integrations/connections')).rejects.toBeInstanceOf(ApiError)
    expect(expired).toHaveBeenCalledTimes(1)
    window.removeEventListener(SESSION_EXPIRED_EVENT, expired)
  })

  it('does not signal session expiry for an invalid sign-in attempt', async () => {
    const expired = vi.fn()
    window.addEventListener(SESSION_EXPIRED_EVENT, expired)
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse({
      error: { code: 'unauthorized', message: 'Invalid email or password' },
    }, 401)))

    await expect(requestJson('/api/v1/auth/sign-in')).rejects.toBeInstanceOf(ApiError)
    expect(expired).not.toHaveBeenCalled()
    window.removeEventListener(SESSION_EXPIRED_EVENT, expired)
  })
})
