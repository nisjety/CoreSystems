import { afterEach, describe, expect, it, vi } from 'vitest'

import { probeSpaceService, SPACE_SERVICE_PROBE_TIMEOUT_MS } from './health'

type FetchCall = { url: string; init: RequestInit }

function stubFetch(respond: (call: FetchCall) => Promise<Response>): FetchCall[] {
  const calls: FetchCall[] = []
  vi.stubGlobal('fetch', vi.fn(async (url: string, init: RequestInit) => {
    const call = { url: String(url), init }
    calls.push(call)
    return respond(call)
  }))
  return calls
}

afterEach(() => {
  vi.useRealTimers()
  vi.unstubAllGlobals()
})

describe('probeSpaceService (WS01-S8, optional)', () => {
  it('GETs the unauthenticated /health with no credentials and no store', async () => {
    const calls = stubFetch(async () => new Response('{"status":"ok"}', { status: 200 }))
    await probeSpaceService()
    expect(calls).toHaveLength(1)
    expect(calls[0]?.url).toBe('/health')
    expect(calls[0]?.init.method).toBe('GET')
    expect(calls[0]?.init.credentials).toBe('omit')
    expect(calls[0]?.init.cache).toBe('no-store')
    expect(calls[0]?.init.headers).toBeUndefined()
  })

  it('reads an answer below 500 as up, whatever the status', async () => {
    stubFetch(async () => new Response('', { status: 200 }))
    expect(await probeSpaceService()).toBe(true)
    stubFetch(async () => new Response('', { status: 404 }))
    expect(await probeSpaceService()).toBe(true)
  })

  it('reads a server error as down', async () => {
    stubFetch(async () => new Response('', { status: 500 }))
    expect(await probeSpaceService()).toBe(false)
    stubFetch(async () => new Response('', { status: 503 }))
    expect(await probeSpaceService()).toBe(false)
  })

  it('reads a request that never gets an answer as down, and never throws', async () => {
    stubFetch(async () => {
      throw new TypeError('Failed to fetch')
    })
    await expect(probeSpaceService()).resolves.toBe(false)
  })

  it('gives up after the timeout and reads it as down', async () => {
    vi.useFakeTimers()
    stubFetch((call) => new Promise<Response>((_resolve, reject) => {
      call.init.signal?.addEventListener('abort', () => reject(new DOMException('aborted', 'AbortError')))
    }))
    const probe = probeSpaceService(1_000)
    await vi.advanceTimersByTimeAsync(1_000)
    await expect(probe).resolves.toBe(false)
    expect(SPACE_SERVICE_PROBE_TIMEOUT_MS).toBeGreaterThan(1_000)
  })
})
