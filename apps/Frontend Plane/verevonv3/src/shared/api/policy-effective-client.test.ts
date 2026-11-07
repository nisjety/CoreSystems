import { afterEach, describe, expect, it, vi } from 'vitest'
import { getEffectivePolicy } from './policy-effective-client'

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('effective policy client', () => {
  it('asks for the thread only and keeps each origin, lock and unavailable item the server gives', async () => {
    const fetchMock = vi.fn(async (_input: RequestInfo | URL, _init?: RequestInit) =>
      new Response(
        JSON.stringify({
          data: {
            items: [
              { key: 'web.mode', value: 'off', origin: 'org', revision: 4, locked: true },
              { key: 'model_access.allow', value: 2, origin: 'platform', locked: false },
              { key: 'model_access', value: null, origin: null, locked: false, unavailable: true },
            ],
          },
        }),
        { status: 200, headers: { 'content-type': 'application/json' } },
      ),
    )
    vi.stubGlobal('fetch', fetchMock)

    const items = await getEffectivePolicy('thread_1')

    expect(String(fetchMock.mock.calls[0]?.[0])).toContain('/api/v1/policy/effective?thread_id=thread_1')
    expect(items).toEqual([
      { key: 'web.mode', value: 'off', origin: 'org', revision: 4, locked: true, unavailable: false },
      { key: 'model_access.allow', value: 2, origin: 'platform', locked: false, unavailable: false },
      { key: 'model_access', value: null, origin: null, locked: false, unavailable: true },
    ])
  })

  it('refuses an origin the view does not name, rather than showing it', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async (_input: RequestInfo | URL, _init?: RequestInit) =>
        new Response(
          JSON.stringify({ data: { items: [{ key: 'web.mode', value: 'off', origin: 'elsewhere', locked: true }] } }),
          { status: 200, headers: { 'content-type': 'application/json' } },
        ),
      ),
    )

    await expect(getEffectivePolicy('thread_1')).rejects.toThrow()
  })
})
