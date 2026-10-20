import { afterEach, describe, expect, it, vi } from 'vitest'
import { translateApiError } from '@/shared/i18n/errors'
import { ApiError, requestForm, requestJson, SESSION_EXPIRED_EVENT } from './http'

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}

describe('gateway HTTP client auth headers', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
    vi.unstubAllEnvs()
  })

  it('does not inject the dev auth bypass unless the environment explicitly opts in', async () => {
    // Pin the opt-in OFF so the assertion is deterministic regardless of any
    // ambient local .env (dev shells set VITE_ALLOW_DEV_AUTH_BYPASS=true).
    vi.stubEnv('VITE_ALLOW_DEV_AUTH_BYPASS', 'false')
    const fetchMock = vi.fn(async () => jsonResponse({ data: { ok: true } }))
    vi.stubGlobal('fetch', fetchMock)

    await requestJson('/api/v1/session/current')

    const [, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit]
    expect((init.headers as Headers).get('Authorization')).toBeNull()
  })

  it('injects the dev auth bypass only when the environment explicitly opts in', async () => {
    vi.stubEnv('VITE_ALLOW_DEV_AUTH_BYPASS', 'true')
    const fetchMock = vi.fn(async () => jsonResponse({ data: { ok: true } }))
    vi.stubGlobal('fetch', fetchMock)

    await requestJson('/api/v1/session/current')

    const [, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit]
    expect((init.headers as Headers).get('Authorization')).toBe('Bearer dev-bypass')
  })

  it('signals session expiry for an unauthorized protected request', async () => {
    const expired = vi.fn()
    window.addEventListener(SESSION_EXPIRED_EVENT, expired)
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse({
      error: { code: 'unauthorized', message: 'Authentication required' },
    }, 401)))

    await expect(requestJson('/api/v1/integrations/connections')).rejects.toBeInstanceOf(ApiError)
    expect(expired).toHaveBeenCalledTimes(1)
    window.removeEventListener(SESSION_EXPIRED_EVENT, expired)
  })

  it('keeps a top-level code beside a string error, as model-gateway sends for no speech', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse({
      error: 'no speech detected in the audio',
      code: 'no_speech_detected',
    }, 422)))

    const failure = await requestJson('/api/v1/ai/dictate', { method: 'POST', body: '{}' }).catch((error: unknown) => error)
    expect(failure).toBeInstanceOf(ApiError)
    expect(failure).toMatchObject({ status: 422, code: 'no_speech_detected', message: 'no speech detected in the audio' })
  })

  it('leaves the code empty for a bare string error', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse({ error: 'upstream failed' }, 502)))

    await expect(requestJson('/api/v1/ai/dictate')).rejects.toMatchObject({ status: 502, code: null, message: 'upstream failed' })
  })

  it('does not signal session expiry for an invalid sign-in attempt', async () => {
    const expired = vi.fn()
    window.addEventListener(SESSION_EXPIRED_EVENT, expired)
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse({
      error: { code: 'unauthorized', message: 'Invalid email or password' },
    }, 401)))

    await expect(requestJson('/api/v1/auth/sign-in')).rejects.toBeInstanceOf(ApiError)
    expect(expired).not.toHaveBeenCalled()
    window.removeEventListener(SESSION_EXPIRED_EVENT, expired)
  })
})

// What every transport does with a failed request: one path back to sign-in for an
// expired session, the wait for a 429, the code for a refusal, and the raw failure
// of a lost connection (never turned into a message here).
describe('gateway HTTP client failure handling', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  const no = (norwegian: string) => norwegian
  const en = (_norwegian: string, english: string) => english
  const fallback = { no: 'Kunne ikke lagre.', en: 'Could not save.' }

  function listenForExpiry(): { expired: ReturnType<typeof vi.fn>; stop: () => void } {
    const expired = vi.fn()
    window.addEventListener(SESSION_EXPIRED_EVENT, expired)
    return { expired, stop: () => window.removeEventListener(SESSION_EXPIRED_EVENT, expired) }
  }

  it('sends an expired session back to sign-in from an upload as well, once', async () => {
    const { expired, stop } = listenForExpiry()
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse({ error: { code: 'unauthorized', message: 'Authentication required' } }, 401)))

    await expect(requestForm('/api/v1/knowledge/upload', new FormData())).rejects.toMatchObject({ status: 401, code: 'unauthorized' })
    expect(expired).toHaveBeenCalledTimes(1)
    stop()
  })

  it('keeps a failed credential check on its form, for a form post as well', async () => {
    const { expired, stop } = listenForExpiry()
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse({ error: { code: 'unauthorized', message: 'Invalid code' } }, 401)))

    await expect(requestForm('/api/v1/auth/two-factor/verify', new FormData())).rejects.toBeInstanceOf(ApiError)
    expect(expired).not.toHaveBeenCalled()
    stop()
  })

  it('does not end the session for a 401 from another service, or for an unanswered check', async () => {
    const { expired, stop } = listenForExpiry()
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse({ code: 'INVALID_OTP', message: 'Invalid OTP' }, 401)))
    await expect(requestJson('/api/v1/settings/profile')).rejects.toMatchObject({ code: 'INVALID_OTP' })

    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse({ error: { code: 'session_verification_unavailable', message: 'retry' } }, 503)))
    const unanswered = await requestJson('/api/v1/settings/profile').catch((error: unknown) => error)
    expect(unanswered).toMatchObject({ status: 503, code: 'session_verification_unavailable' })
    expect(translateApiError(unanswered, no, fallback)).toMatch(/økten er ikke endret/)

    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse({}, 500)))
    await expect(requestJson('/api/v1/settings/profile')).rejects.toMatchObject({ status: 500 })
    expect(expired).not.toHaveBeenCalled()
    stop()
  })

  it('shows the wait of a 429 from the gateway, from the body and from the header', async () => {
    // The typed envelope the rate limiter answers with now.
    vi.stubGlobal('fetch', vi.fn(async () => new Response(
      JSON.stringify({ error: { code: 'rate_limited', message: 'Too many requests. Try again shortly.', retryAfterSeconds: 12 } }),
      { status: 429, headers: { 'Content-Type': 'application/json', 'Retry-After': '12' } },
    )))
    const limited = await requestJson('/api/v1/search/web').catch((error: unknown) => error)
    expect(limited).toMatchObject({ status: 429, code: 'rate_limited', retryAfterSeconds: 12 })
    expect(translateApiError(limited, no, fallback)).toBe('Det kommer for mange forespørsler akkurat nå. Prøv igjen om 12 sekunder.')
    expect(translateApiError(limited, en, fallback)).toBe('There are too many requests right now. Try again in 12 seconds.')

    // A plain-text 429 (an older gateway, or a proxy in front of it) still shows the wait from the header.
    vi.stubGlobal('fetch', vi.fn(async () => new Response('rate limit exceeded', { status: 429, headers: { 'Retry-After': '7' } })))
    const plain = await requestJson('/api/v1/search/web').catch((error: unknown) => error)
    expect(plain).toMatchObject({ status: 429, code: null, retryAfterSeconds: 7 })
    expect(translateApiError(plain, no, fallback)).toBe('Det kommer for mange forespørsler akkurat nå. Prøv igjen om 7 sekunder.')

    // An upload gets the same.
    const upload = await requestForm('/api/v1/knowledge/upload', new FormData()).catch((error: unknown) => error)
    expect(upload).toMatchObject({ status: 429, retryAfterSeconds: 7 })
  })

  it('leaves a lost connection as the raw failure, which the copy turns into "check your network"', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => { throw new TypeError('Failed to fetch') }))

    const lost = await requestJson('/api/v1/session/current').catch((error: unknown) => error)
    expect(lost).toBeInstanceOf(TypeError)
    expect(translateApiError(lost, no, fallback)).toBe('Fikk ikke kontakt med tjenesten. Sjekk nettverket ditt og prøv igjen.')
    expect(translateApiError(lost, en, fallback)).toBe('Could not reach the service. Check your network and try again.')
  })

  it('gives a typed refusal its own sentence, and a bare 5xx the sentence of the caller', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => jsonResponse({ error: { code: 'space_not_found', message: 'Space is not available.' } }, 404)))
    const missing = await requestJson('/api/v1/spaces/x').catch((error: unknown) => error)
    expect(translateApiError(missing, no, fallback)).toMatch(/Fant ikke dette rommet/)

    vi.stubGlobal('fetch', vi.fn(async () => new Response('<html>Bad gateway</html>', { status: 502 })))
    const bare = await requestJson('/api/v1/spaces/x').catch((error: unknown) => error)
    expect(bare).toMatchObject({ status: 502, code: null })
    expect(translateApiError(bare, no, fallback)).toBe('Kunne ikke lagre.')
    expect(translateApiError(bare, no, fallback)).not.toMatch(/html|Bad gateway|502/i)
  })
})
