import { afterEach, describe, expect, it, vi } from 'vitest'
import { ApiError, extractError, requestJson } from './http'

function jsonResponse(data: unknown, status: number): Response {
  return new Response(JSON.stringify(data), { status, headers: { 'Content-Type': 'application/json' } })
}

describe('the feature a zero-retention refusal names', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('is read from the error envelope', () => {
    expect(extractError({ error: { code: 'zdr_feature_not_supported', message: 'x', feature: 'web_search' } }, 412)).toEqual({
      code: 'zdr_feature_not_supported',
      message: 'x',
      feature: 'web_search',
    })
  })

  it('is absent when the body names none, so an ordinary error keeps its exact shape', () => {
    expect(extractError({ error: { code: 'forbidden', message: 'no' } }, 403)).toEqual({ code: 'forbidden', message: 'no' })
  })

  it('reaches the ApiError a JSON request throws', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse({
      error: { code: 'zdr_feature_not_supported', message: 'x', feature: 'browser_run' },
    }, 412)))
    const failure = await requestJson('/api/v1/browser/runs', { method: 'POST' }).catch((err: unknown) => err)
    expect(failure).toBeInstanceOf(ApiError)
    expect(failure).toMatchObject({ status: 412, code: 'zdr_feature_not_supported', feature: 'browser_run' })
  })

  it('is null on an ApiError whose body names no feature', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse({ error: { code: 'zdr_unavailable', message: 'x' } }, 412)))
    const failure = await requestJson('/api/v1/browser/runs', { method: 'POST' }).catch((err: unknown) => err)
    expect(failure).toMatchObject({ code: 'zdr_unavailable' })
    expect((failure as ApiError).feature).toBeNull()
  })
})
