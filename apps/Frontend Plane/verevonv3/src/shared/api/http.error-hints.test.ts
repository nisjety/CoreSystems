import { afterEach, describe, expect, it, vi } from 'vitest'
import { ApiError, apiErrorFromResponse, extractError, requestForm, requestJson } from './http'

function jsonResponse(data: unknown, status: number, headers: Record<string, string> = {}): Response {
  return new Response(JSON.stringify(data), { headers: { 'Content-Type': 'application/json', ...headers }, status })
}

describe('ApiError hints survive the transport', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('an envelope with only a code and a message has no hints', () => {
    const failure = apiErrorFromResponse({ error: { code: 'forbidden', message: 'no' } }, new Response(null, { status: 403 }))
    expect(failure).toBeInstanceOf(ApiError)
    expect(failure).toMatchObject({
      status: 403,
      code: 'forbidden',
      message: 'no',
      details: null,
      retryAfterSeconds: null,
      resumable: false,
      blockReason: null,
    })
  })

  it('carries details, retryAfterSeconds, resumable and blockReason from the typed envelope', () => {
    const failure = apiErrorFromResponse(
      {
        error: {
          code: 'browser_site_blocked',
          message: 'x',
          retryAfterSeconds: 9,
          resumable: true,
          blockReason: 'waf_interstitial',
          details: { control_epoch: 2 },
        },
      },
      new Response(null, { status: 403 }),
    )
    expect(failure).toMatchObject({
      retryAfterSeconds: 9,
      resumable: true,
      blockReason: 'waf_interstitial',
      details: { control_epoch: 2 },
    })
  })

  it('extractError still answers code and message for every body shape', () => {
    expect(extractError({ error: 'plain', code: 'c' }, 500)).toMatchObject({ code: 'c', message: 'plain' })
    expect(extractError({ code: 'EMAIL_NOT_VERIFIED', message: 'verify' }, 403)).toMatchObject({ code: 'EMAIL_NOT_VERIFIED', message: 'verify' })
    expect(extractError(null, 502)).toMatchObject({ code: null, message: 'Request failed (502)' })
    // A details array or string is not a details object.
    expect(extractError({ error: { code: 'x', details: ['a'] } }, 400).hints.details).toBeNull()
    expect(extractError({ error: { code: 'x', details: 'a' } }, 400).hints.details).toBeNull()
  })

  it('requestJson and requestForm both keep the Retry-After header of a 429', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse({ error: { code: 'rate_limited', message: 'x' } }, 429, { 'Retry-After': '21' })))
    const json = await requestJson('/api/v1/x').catch((error: unknown) => error)
    expect(json).toMatchObject({ status: 429, code: 'rate_limited', retryAfterSeconds: 21 })
    const form = await requestForm('/api/v1/x', new FormData()).catch((error: unknown) => error)
    expect(form).toMatchObject({ status: 429, code: 'rate_limited', retryAfterSeconds: 21 })
  })

  it('works with a response whose headers are missing (a hand-made fetch mock)', () => {
    const failure = apiErrorFromResponse({ error: { code: 'x', message: 'y' } }, { status: 500 } as unknown as Response)
    expect(failure).toMatchObject({ status: 500, code: 'x', retryAfterSeconds: null })
  })
})
