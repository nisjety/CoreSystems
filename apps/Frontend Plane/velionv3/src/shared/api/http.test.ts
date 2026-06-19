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
  })

  it('does not inject the dev auth bypass unless the environment explicitly opts in', async () => {
    const fetchMock = vi.fn(async () => jsonResponse({ data: { ok: true } }))
    vi.stubGlobal('fetch', fetchMock)

    await requestJson('/api/v1/session/current')

    const [, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit]
    expect((init.headers as Headers).get('Authorization')).toBeNull()
  })
})
