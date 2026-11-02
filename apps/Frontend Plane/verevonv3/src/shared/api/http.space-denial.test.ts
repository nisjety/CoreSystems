import { afterEach, describe, expect, it, vi } from 'vitest'
import { ApiError, extractError, requestForm, requestJson } from './http'
import { readSseStream } from './sse'

// WS03-S7: a typed Space denial is `{ error: { code, message, details } }`. The `details`
// (reason, resolvable_by and a few allowlisted keys) must survive every transport that
// builds an ApiError, so the presenter can say why; a body without details gives null.

function jsonResponse(data: unknown, status: number): Response {
  return new Response(JSON.stringify(data), { status, headers: { 'Content-Type': 'application/json' } })
}

const denial = {
  error: {
    code: 'forbidden',
    message: 'internal wording',
    details: { reason: 'space_role_below_floor', resolvable_by: 'space_manager' },
  },
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('extractError details', () => {
  it('reads details from the error envelope', () => {
    const extracted = extractError(denial, 403)
    expect(extracted).toMatchObject({ code: 'forbidden', message: 'internal wording' })
    expect(extracted.hints.details).toEqual({ reason: 'space_role_below_floor', resolvable_by: 'space_manager' })
  })

  it('leaves details out when the body has none, so an ordinary error keeps its shape', () => {
    const extracted = extractError({ error: { code: 'forbidden', message: 'no' } }, 403)
    expect(extracted).toMatchObject({ code: 'forbidden', message: 'no' })
    expect(extracted.hints.details).toBeNull()
  })

  it('ignores details that are not an object', () => {
    for (const details of ['oops', 7, null, ['a']]) {
      expect(extractError({ error: { code: 'x', message: 'm', details } }, 403).hints.details).toBeNull()
    }
  })
})

describe('ApiError details', () => {
  it('defaults to null', () => {
    expect(new ApiError('m', 403, 'forbidden').details).toBeNull()
  })

  it('is carried by requestJson', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse(denial, 403)))
    const failure = await requestJson('/api/v1/spaces/s1/posts', { method: 'POST' }).catch((err: unknown) => err)
    expect(failure).toBeInstanceOf(ApiError)
    expect(failure).toMatchObject({
      status: 403,
      code: 'forbidden',
      details: { reason: 'space_role_below_floor', resolvable_by: 'space_manager' },
    })
  })

  it('is carried by requestForm', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse(denial, 403)))
    const failure = await requestForm('/api/v1/spaces/s1/files', new FormData()).catch((err: unknown) => err)
    expect(failure).toBeInstanceOf(ApiError)
    expect(failure).toMatchObject({ status: 403, details: { reason: 'space_role_below_floor' } })
  })

  it('is carried by the SSE connect error', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse(denial, 403)))
    const onError = vi.fn()
    await readSseStream('/api/v1/chat/stream', { method: 'POST', body: '{}' }, vi.fn(), onError)
    expect(onError).toHaveBeenCalledTimes(1)
    const failure = onError.mock.calls[0]?.[0]
    expect(failure).toBeInstanceOf(ApiError)
    expect(failure).toMatchObject({ status: 403, code: 'forbidden', details: { reason: 'space_role_below_floor' } })
  })

  it('is null when the body carries no details (requestJson, requestForm and SSE)', async () => {
    const plain = { error: { code: 'forbidden', message: 'no' } }
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse(plain, 403)))
    const json = await requestJson('/api/v1/x').catch((err: unknown) => err)
    expect((json as ApiError).details).toBeNull()
    const form = await requestForm('/api/v1/x', new FormData()).catch((err: unknown) => err)
    expect((form as ApiError).details).toBeNull()
    const onError = vi.fn()
    await readSseStream('/api/v1/x', {}, vi.fn(), onError)
    expect((onError.mock.calls[0]?.[0] as ApiError).details).toBeNull()
  })

  it('is null for a non-JSON error body', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('<html>bad gateway</html>', { status: 502 })))
    const failure = await requestJson('/api/v1/x').catch((err: unknown) => err)
    expect(failure).toMatchObject({ status: 502, details: null })
  })
})
