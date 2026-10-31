import { afterEach, describe, expect, it, vi } from 'vitest'
import { ApiError, SESSION_EXPIRED_EVENT } from './http'
import {
  browserErrorMessage,
  createBrowserTab,
  describeBrowserError,
  runBrowserAction,
  setBrowserControlMode,
} from './browser-client'

const no = (norwegian: string) => norwegian
const en = (_norwegian: string, english: string) => english

function reply(status: number, body: unknown, headers: Record<string, string> = {}): Response {
  return new Response(JSON.stringify(body), {
    headers: { 'Content-Type': 'application/json', ...headers },
    status,
  })
}

async function failureOf(status: number, body: unknown, headers?: Record<string, string>): Promise<ApiError> {
  vi.stubGlobal('fetch', vi.fn(async () => reply(status, body, headers)))
  const failure = await createBrowserTab('org-1', 'sess-1').catch((error: unknown) => error)
  expect(failure).toBeInstanceOf(ApiError)
  return failure as ApiError
}

describe('browser client error handling', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('keeps the code, the retry hint and the typed details of the gateway envelope', async () => {
    const failure = await failureOf(
      429,
      { error: { code: 'browser_capacity_reached', message: 'Browser capacity is full. Try again shortly.', retryAfterSeconds: 15 } },
      { 'Retry-After': '15' },
    )
    expect(failure).toMatchObject({ status: 429, code: 'browser_capacity_reached', retryAfterSeconds: 15, resumable: false })
    expect(describeBrowserError(failure, no)).toMatchObject({
      kind: 'rate_limited',
      message: 'Det er for mange nettleserøkter i gang akkurat nå. Prøv igjen om 15 sekunder.',
    })
  })

  it('reads Retry-After from the header when the body has no hint, and ignores a hint that is not sane', async () => {
    const fromHeader = await failureOf(429, { error: { code: 'rate_limited', message: 'slow down' } }, { 'Retry-After': '42' })
    expect(fromHeader.retryAfterSeconds).toBe(42)
    expect(browserErrorMessage(fromHeader, en)).toMatch(/Try again in 42 seconds\.$/)

    for (const header of ['0', '999999', 'Wed, 21 Oct 2026 07:28:00 GMT', 'soon']) {
      const failure = await failureOf(429, { error: { code: 'rate_limited', message: 'x' } }, { 'Retry-After': header })
      expect(failure.retryAfterSeconds, header).toBeNull()
    }
    // The body's own hint wins over the header.
    const both = await failureOf(
      429,
      { error: { code: 'browser_tab_limit_reached', message: 'x', retryAfterSeconds: 7 } },
      { 'Retry-After': '30' },
    )
    expect(both.retryAfterSeconds).toBe(7)
  })

  it('reads Quarry-shaped details when the gateway passed the body through', async () => {
    const failure = await failureOf(404, {
      error: {
        code: 'NOT_FOUND',
        message: 'run not found',
        details: { reason: 'run_not_live', resumable: true, resume_run_id: 'run_abc', retry_after_seconds: 3 },
      },
    })
    expect(failure).toMatchObject({ code: 'NOT_FOUND', resumable: true, retryAfterSeconds: 3 })
    expect(failure.details).toMatchObject({ reason: 'run_not_live' })
    expect(describeBrowserError(failure, no).resumable).toBe(true)
  })

  it('keeps the typed bot-wall reason from the envelope and from Quarry details', async () => {
    const gateway = await failureOf(403, { error: { code: 'browser_site_blocked', message: 'x', blockReason: 'captcha_widget', recovery: 'exhausted' } })
    expect(gateway.blockReason).toBe('captcha_widget')
    expect(describeBrowserError(gateway, no)).toMatchObject({ kind: 'blocked' })
    expect(browserErrorMessage(gateway, no)).toMatch(/bekrefter at du er et menneske/)

    const passthrough = await failureOf(403, {
      error: { code: 'CHALLENGE_DETECTED', message: 'blocked https://secret.example', details: { block_reason: 'waf_interstitial', host: 'secret.example' } },
    })
    expect(passthrough.blockReason).toBe('waf_interstitial')
    expect(browserErrorMessage(passthrough, no)).not.toMatch(/secret\.example/)
  })

  it('handles 401, 403, 409 take-over, 410, 429 and 5xx distinctly', async () => {
    // [status, body, kind, the exact Norwegian sentence]. The eight sentences are
    // pairwise distinct: a status class never borrows another one's wording.
    const cases: Array<[number, unknown, string, string]> = [
      [401, { error: { code: 'unauthenticated', message: 'Authentication required' } }, 'unauthenticated', 'Du må logge inn på nytt.'],
      [403, { error: { code: 'forbidden', message: 'no' } }, 'forbidden', 'Du har ikke tilgang til å gjøre dette.'],
      [
        409,
        { error: { code: 'browser_human_takeover_active', message: 'x', details: { control_epoch: 3 } } },
        'takeover',
        'En person har overtatt styringen av denne nettleserøkten.',
      ],
      [
        409,
        { error: { code: 'USER_TOOK_OVER', message: 'x' } },
        'takeover',
        'En person har overtatt nettleseren, så agenten kan ikke handle akkurat nå. Vent til styringen er gitt tilbake.',
      ],
      [
        410,
        { error: { code: 'browser_session_reaped', message: 'x' } },
        'session_lost',
        'Denne nettleserøkten ble lukket fordi den var inaktiv. Start en ny økt for å fortsette.',
      ],
      [
        429,
        { error: { code: 'browser_tab_limit_reached', message: 'x', retryAfterSeconds: 12 } },
        'rate_limited',
        'Nettleseren har for mange faner åpne. Lukk en fane du ikke trenger. Prøv igjen om 12 sekunder.',
      ],
      [
        500,
        { error: { code: 'browser_session_failed', message: 'Quarry browser session is not available.' } },
        'server',
        'Nettleserøkten feilet. Prøv igjen.',
      ],
      [
        503,
        { error: { code: 'upstream_unavailable', message: 'The upstream service is unavailable.' } },
        'server',
        'Tjenesten er utilgjengelig akkurat nå. Prøv igjen om litt.',
      ],
    ]
    const messages: string[] = []
    for (const [status, body, kind, sentence] of cases) {
      const failure = await failureOf(status, body)
      const description = describeBrowserError(failure, no)
      expect(description.kind, `${status} ${kind}`).toBe(kind)
      expect(description.message, `${status} ${kind}`).toBe(sentence)
      messages.push(description.message)
      // Whatever the case, the English ops text of the response is not what is shown.
      expect(description.message).not.toMatch(/Authentication required|Quarry|upstream service/)
    }
    expect(new Set(messages).size).toBe(cases.length)
    expect(new Set(cases.map(([, , kind]) => kind)).size).toBe(6)
  })

  it('shows the generic message for an unknown code, never the upstream message', async () => {
    const failure = await failureOf(400, {
      error: { code: 'brand_new_quarry_code', message: 'internal: postgres://db.internal/quarry failed for org_123' },
    })
    for (const tr of [no, en]) {
      const message = browserErrorMessage(failure, tr)
      expect(message).not.toMatch(/postgres|db\.internal|org_123|brand_new_quarry_code/)
      expect(message.length).toBeGreaterThan(10)
    }
    // A caller's own fallback is used when it gave one.
    expect(browserErrorMessage(failure, en, { no: 'x', en: 'The browser action could not be completed.' })).toBe(
      'The browser action could not be completed.',
    )
    // A failure with no body at all is the same.
    const empty = await failureOf(502, null)
    expect(browserErrorMessage(empty, no)).not.toMatch(/Request failed/)
  })

  it('reports a request that never got an answer as a network failure, not as its raw text', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => {
      throw new TypeError('Failed to fetch')
    }))
    const failure = await runBrowserAction('org-1', 'sess-1', { type: 'back' }).catch((error: unknown) => error)
    expect(describeBrowserError(failure, no).kind).toBe('network')
    expect(browserErrorMessage(failure, en)).not.toMatch(/Failed to fetch/)
  })

  it('signals session expiry only for a gateway unauthorized, and still carries the typed error', async () => {
    const expired = vi.fn()
    window.addEventListener(SESSION_EXPIRED_EVENT, expired)
    try {
      vi.stubGlobal('fetch', vi.fn(async () => reply(401, { error: { code: 'unauthorized', message: 'Authentication required' } })))
      const failure = await setBrowserControlMode('org-1', 'sess-1', 'human_takeover').catch((error: unknown) => error)
      expect(expired).toHaveBeenCalledTimes(1)
      expect(describeBrowserError(failure, no)).toMatchObject({ kind: 'unauthenticated', message: 'Du må logge inn på nytt.' })
    } finally {
      window.removeEventListener(SESSION_EXPIRED_EVENT, expired)
    }
  })
})
