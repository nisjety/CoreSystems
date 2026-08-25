import { afterEach, describe, expect, it, vi } from 'vitest'
import { getRun, getRunWatchStatus, unwatchRun, watchRun } from './runs-client'

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    status,
    headers: { 'Content-Type': 'application/json' },
  })
}

describe('getRun privacy-tier provenance', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('parses tier + residency from run provenance (top level or metadata)', async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse({
      run: {
        run_id: 'run_1',
        status: 'completed',
        goal: 'g',
        checkpoint_index: 0,
        steps_completed: 1,
        input_tokens: 1,
        output_tokens: 2,
        privacy_tier: 'sovereign',
        residency: 'norway-east',
      },
    }))
    vi.stubGlobal('fetch', fetchMock)

    const run = await getRun('run_1')

    expect(run).toEqual(expect.objectContaining({
      runId: 'run_1',
      privacyTier: 'sovereign',
      residency: 'norway-east',
    }))
  })

  it('falls back to metadata.privacy_tier beside the existing residency fallback', async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse({
      run: {
        run_id: 'run_2',
        status: 'completed',
        goal: 'g',
        metadata: { privacy_tier: 'zdr_contractual', region: 'eu-central-1' },
      },
    }))
    vi.stubGlobal('fetch', fetchMock)

    const run = await getRun('run_2')

    expect(run?.privacyTier).toBe('zdr_contractual')
    expect(run?.residency).toBe('eu-central-1')
  })

  it('degrades unknown tier values to undefined — never a fabricated claim', async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse({
      run: {
        run_id: 'run_3',
        status: 'completed',
        goal: 'g',
        privacy_tier: 'fort_knox',
      },
    }))
    vi.stubGlobal('fetch', fetchMock)

    const run = await getRun('run_3')

    expect(run?.privacyTier).toBeUndefined()
  })
})

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
