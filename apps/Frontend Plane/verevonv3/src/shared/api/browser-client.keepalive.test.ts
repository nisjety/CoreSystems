import { afterEach, describe, expect, it, vi } from 'vitest'
import { keepAliveBrowserSession } from './browser-client'

describe('keepAliveBrowserSession (WS8.D)', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('posts to the session keep-alive route and returns only the expiry', async () => {
    const fetchMock = vi.fn(async () =>
      new Response(JSON.stringify({ data: { expiresAt: '2030-01-01T00:15:00Z' } }), {
        status: 200,
        headers: { 'content-type': 'application/json' },
      }),
    )
    vi.stubGlobal('fetch', fetchMock)

    const result = await keepAliveBrowserSession('org-1', 'run-1')

    expect(result).toEqual({ expiresAt: '2030-01-01T00:15:00Z' })
    const [url, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit]
    expect(url).toContain('/api/v1/browser/sessions/run-1/keepalive')
    expect(init.method).toBe('POST')
  })

  it('surfaces a 409 browser_session_expired as a thrown error', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        new Response(JSON.stringify({ error: { code: 'browser_session_expired', message: 'ended' } }), {
          status: 409,
          headers: { 'content-type': 'application/json' },
        }),
      ),
    )

    await expect(keepAliveBrowserSession('org-1', 'run-1')).rejects.toBeDefined()
  })
})
