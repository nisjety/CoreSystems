import { afterEach, describe, expect, it, vi } from 'vitest'
import { getRunWatchStatus, unwatchRun, watchRun } from './runs-client'

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    status,
    headers: { 'Content-Type': 'application/json' },
  })
}

describe('watchRun', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('POSTs to the encoded run watchers endpoint and normalizes the response', async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse({ watching: true }))
    vi.stubGlobal('fetch', fetchMock)

    const result = await watchRun('run 1')

    const [path, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(path).toBe('/api/v1/runs/run%201/watchers')
    expect(init.method).toBe('POST')
    expect(result).toEqual({ watching: true })
  })

  it('never claims watching from a malformed payload', async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse({ watching: 'yes' }))
    vi.stubGlobal('fetch', fetchMock)

    const result = await watchRun('run-1')

    expect(result).toEqual({ watching: false })
  })

  it('propagates an upstream failure rather than silently succeeding', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      jsonResponse({ error: { code: 'run_not_found', message: 'run not found' } }, 404),
    )
    vi.stubGlobal('fetch', fetchMock)

    await expect(watchRun('gone')).rejects.toThrow('run not found')
  })
})

describe('unwatchRun', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('sends a DELETE against the encoded run watchers endpoint', async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse({}))
    vi.stubGlobal('fetch', fetchMock)

    await unwatchRun('run 1')

    const [path, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(path).toBe('/api/v1/runs/run%201/watchers')
    expect(init.method).toBe('DELETE')
  })
})

describe('getRunWatchStatus', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('GETs the encoded run watchers endpoint and normalizes the response', async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse({ watching: false }))
    vi.stubGlobal('fetch', fetchMock)

    const result = await getRunWatchStatus('run 1')

    const [path, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(path).toBe('/api/v1/runs/run%201/watchers')
    expect(init?.method ?? 'GET').toBe('GET')
    expect(result).toEqual({ watching: false })
  })

  it('defaults to not-watching when the field is absent', async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse({}))
    vi.stubGlobal('fetch', fetchMock)

    const result = await getRunWatchStatus('run-1')

    expect(result).toEqual({ watching: false })
  })
})
