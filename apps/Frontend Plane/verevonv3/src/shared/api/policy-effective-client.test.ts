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

  it('keeps a withheld count and an admin list as the server sends them', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async (_input: RequestInfo | URL, _init?: RequestInit) =>
        new Response(
          JSON.stringify({
            data: {
              items: [
                { key: 'model_access.allow', value: null, origin: 'org', locked: true, redacted: true, count: 2 },
                {
                  key: 'browser_sites.rules',
                  value: null,
                  origin: 'org',
                  locked: true,
                  values: ['example.com (read)'],
                },
              ],
            },
          }),
          { status: 200, headers: { 'content-type': 'application/json' } },
        ),
      ),
    )

    const items = await getEffectivePolicy('thread_1')

    expect(items[0]).toMatchObject({ key: 'model_access.allow', value: null, redacted: true, count: 2 })
    expect(items[1]).toMatchObject({ key: 'browser_sites.rules', values: ['example.com (read)'] })
  })

  it('refuses a list that holds anything but text', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async (_input: RequestInfo | URL, _init?: RequestInit) =>
        new Response(
          JSON.stringify({
            data: {
              items: [{ key: 'model_access.allow', value: null, origin: 'org', locked: false, values: ['ok', 3] }],
            },
          }),
          { status: 200, headers: { 'content-type': 'application/json' } },
        ),
      ),
    )

    await expect(getEffectivePolicy('thread_1')).rejects.toThrow()
  })

  it('bounds an entry in characters, as the panel and the server do, not in UTF-16 units', async () => {
    const answer = (entry: string) =>
      vi.fn(async (_input: RequestInfo | URL, _init?: RequestInit) =>
        new Response(
          JSON.stringify({
            data: {
              items: [{ key: 'browser_sites.rules', value: null, origin: 'org', locked: false, values: [entry] }],
            },
          }),
          { status: 200, headers: { 'content-type': 'application/json' } },
        ),
      )
    // 200 code points are 400 UTF-16 units: the bound is the characters a person reads.
    vi.stubGlobal('fetch', answer('😀'.repeat(200)))
    await expect(getEffectivePolicy('thread_1')).resolves.toMatchObject([{ key: 'browser_sites.rules' }])
    vi.stubGlobal('fetch', answer('😀'.repeat(201)))
    await expect(getEffectivePolicy('thread_1')).rejects.toThrow()
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
