import { afterEach, describe, expect, it, vi } from 'vitest'
import { describeApiError, ERROR_COPY } from '@/shared/i18n/errors'
import {
  controlBrowserAiRun,
  createAiRunStartKeyring,
  isValidAiRunIdempotencyKey,
  startBrowserAiRun,
  type StartBrowserAiRunParams,
} from './browser-run-client'

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify({ data }), {
    status,
    headers: { 'Content-Type': 'application/json' },
  })
}

describe('startBrowserAiRun', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('posts to the session ai-runs route with camelCase params and org header', async () => {
    // The REAL wire response is snake_case — model-gateway's `browser_run.rs`
    // constructs `json!({ "run_id", "thread_id", "plan_id" })` directly and
    // the Verevon gateway forwards it verbatim (confirmed live: a prior
    // version of this test mocked a camelCase response here, which let a
    // real snake_case/camelCase mismatch bug ship silently — every AI-loop
    // run through the SPA started successfully server-side but the client
    // never resolved a `runId` to stream events for, so it looked stuck
    // forever with no visible error. Regression-tested end-to-end by
    // `tests/e2e/browser-workspace-hitl.spec.ts`).
    const fetchMock = vi.fn().mockResolvedValue(
      jsonResponse({ run_id: 'run_1', thread_id: 'thread_1', plan_id: 'plan_1' }),
    )
    vi.stubGlobal('fetch', fetchMock)

    const result = await startBrowserAiRun('org_1', 'session_1', {
      goal: 'find the cheapest flight',
      allowedDomains: ['norwegian.com'],
      maxSteps: 10,
      maxRuntimeSeconds: 90,
    })

    const [path, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(path).toContain('/api/v1/browser/sessions/session_1/ai-runs')
    expect(init.method).toBe('POST')
    expect((init.headers as Headers).get('x-verevon-org-id')).toBe('org_1')
    expect(JSON.parse(init.body as string)).toEqual({
      goal: 'find the cheapest flight',
      allowedDomains: ['norwegian.com'],
      maxSteps: 10,
      maxRuntimeS: 90,
      stopCriteria: undefined,
      requireApproval: undefined,
      maxCostUsd: undefined,
    })
    expect(result).toEqual({ runId: 'run_1', threadId: 'thread_1', planId: 'plan_1' })
  })

  it('explicitly requests a session binding when required', async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse({ run_id: 'run_1', thread_id: 'thread_1', plan_id: 'plan_1' }))
    vi.stubGlobal('fetch', fetchMock)
    await startBrowserAiRun('org_1', 'session_1', { goal: 'navigate', requireAttached: true })
    const [, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(JSON.parse(init.body as string)).toMatchObject({ goal: 'navigate', requireAttached: true })
  })
})

describe('assistant interactive browser policy', () => {
  afterEach(() => vi.unstubAllGlobals())
  it('sends the server-owned interactive policy without plan mode', async () => {
    const mock = vi.fn().mockResolvedValue(jsonResponse({ run_id: 'run_1', thread_id: 'thread_1', plan_id: 'plan_1' }))
    vi.stubGlobal('fetch', mock)
    await startBrowserAiRun('org_1', 'session_1', { goal: 'inspect', requireAttached: true, assistantInteractive: true })
    const [, init] = mock.mock.calls[0] as [string, RequestInit]
    expect(JSON.parse(init.body as string)).toMatchObject({ requireAttached: true, assistantInteractive: true })
    expect(JSON.parse(init.body as string)).not.toHaveProperty('planMode')
  })
})

const STARTED = { run_id: 'run_1', thread_id: 'thread_1', plan_id: 'plan_1' }

function failure(status: number, code: string): Response {
  return new Response(JSON.stringify({ error: { code, message: 'upstream text that is never shown' } }), {
    status,
    headers: { 'Content-Type': 'application/json' },
  })
}

function sentKey(fetchMock: ReturnType<typeof vi.fn>, call: number): string | null {
  const [, init] = fetchMock.mock.calls[call] as [string, RequestInit]
  return (init.headers as Headers).get('Idempotency-Key')
}

describe('startBrowserAiRun idempotency key', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('sends the key as an Idempotency-Key header, next to the org header', async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse(STARTED))
    vi.stubGlobal('fetch', fetchMock)

    await startBrowserAiRun('org_1', 'session_1', { goal: 'g' }, undefined, { idempotencyKey: 'ai-run-abc:1.2_3' })

    const [, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect((init.headers as Headers).get('Idempotency-Key')).toBe('ai-run-abc:1.2_3')
    expect((init.headers as Headers).get('x-verevon-org-id')).toBe('org_1')
  })

  it('sends no Idempotency-Key header when none is given', async () => {
    const fetchMock = vi.fn().mockImplementation(async () => jsonResponse(STARTED))
    vi.stubGlobal('fetch', fetchMock)

    await startBrowserAiRun('org_1', 'session_1', { goal: 'g' })
    await startBrowserAiRun('org_1', 'session_1', { goal: 'g' }, undefined, {})

    expect(sentKey(fetchMock, 0)).toBeNull()
    expect(sentKey(fetchMock, 1)).toBeNull()
  })

  it.each(['', ' ', 'has space', 'semi;colon', 'slash/slash', 'ø', 'x'.repeat(129)])(
    'refuses the malformed key %j before any request is made',
    async (key) => {
      const fetchMock = vi.fn().mockResolvedValue(jsonResponse(STARTED))
      vi.stubGlobal('fetch', fetchMock)

      await expect(startBrowserAiRun('org_1', 'session_1', { goal: 'g' }, undefined, { idempotencyKey: key })).rejects.toThrow()
      expect(fetchMock).not.toHaveBeenCalled()
    },
  )

  it('accepts a key of exactly 128 characters', () => {
    expect(isValidAiRunIdempotencyKey('x'.repeat(128))).toBe(true)
    expect(isValidAiRunIdempotencyKey('x'.repeat(129))).toBe(false)
  })
})

describe('the AI-run start keyring', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  // What the composer does around every start.
  async function start(
    keys: ReturnType<typeof createAiRunStartKeyring>,
    params: StartBrowserAiRunParams,
    sessionId = 'session_1',
  ) {
    try {
      const result = await startBrowserAiRun('org_1', sessionId, params, undefined, {
        idempotencyKey: keys.keyFor(sessionId, params),
      })
      keys.succeeded()
      return result
    } catch (thrown) {
      keys.failed(thrown)
      throw thrown
    }
  }

  it('hands out keys that the gateway accepts', () => {
    const key = createAiRunStartKeyring().keyFor('session_1', { goal: 'g' })
    expect(isValidAiRunIdempotencyKey(key)).toBe(true)
  })

  it.each([
    ['the connection drops', () => Promise.reject(new TypeError('Failed to fetch'))],
    ['the gateway answers 503 browser_idempotency_unverified', () => Promise.resolve(failure(503, 'browser_idempotency_unverified'))],
    ['the gateway answers 502', () => Promise.resolve(failure(502, 'model_gateway_unreachable'))],
    ['the request times out with 408', () => Promise.resolve(failure(408, 'request_timeout'))],
  ])('reuses one key for the retry of a start when %s', async (_label, firstAnswer) => {
    const fetchMock = vi.fn().mockImplementationOnce(firstAnswer).mockResolvedValue(jsonResponse(STARTED))
    vi.stubGlobal('fetch', fetchMock)
    const keys = createAiRunStartKeyring()

    await expect(start(keys, { goal: 'find the cheapest flight' })).rejects.toBeDefined()
    await expect(start(keys, { goal: 'find the cheapest flight' })).resolves.toMatchObject({ runId: 'run_1' })

    expect(sentKey(fetchMock, 0)).toMatch(/^ai-run-/)
    expect(sentKey(fetchMock, 1)).toBe(sentKey(fetchMock, 0))
  })

  it('uses a fresh key for the next submit once a start was accepted', async () => {
    const fetchMock = vi.fn().mockImplementation(async () => jsonResponse(STARTED))
    vi.stubGlobal('fetch', fetchMock)
    const keys = createAiRunStartKeyring()

    await start(keys, { goal: 'same goal' })
    await start(keys, { goal: 'same goal' })

    expect(sentKey(fetchMock, 0)).not.toBeNull()
    expect(sentKey(fetchMock, 1)).not.toBeNull()
    expect(sentKey(fetchMock, 1)).not.toBe(sentKey(fetchMock, 0))
  })

  it.each([
    [409, 'browser_idempotency_key_reused'],
    [409, 'browser_idempotency_key_run_ended'],
    [409, 'browser_session_grant_inactive'],
    [400, 'invalid_idempotency_key'],
    [403, 'forbidden'],
    [429, 'rate_limited'],
  ])('drops the key after a definite %i %s, so the next start is a new one', async (status, code) => {
    const fetchMock = vi.fn().mockResolvedValueOnce(failure(status, code)).mockResolvedValue(jsonResponse(STARTED))
    vi.stubGlobal('fetch', fetchMock)
    const keys = createAiRunStartKeyring()

    await expect(start(keys, { goal: 'g' })).rejects.toMatchObject({ status, code })
    await start(keys, { goal: 'g' })

    expect(sentKey(fetchMock, 1)).not.toBe(sentKey(fetchMock, 0))
  })

  it('does not carry a held key over to a different goal or a different session', async () => {
    const fetchMock = vi
      .fn()
      .mockRejectedValueOnce(new TypeError('Failed to fetch'))
      .mockRejectedValueOnce(new TypeError('Failed to fetch'))
      .mockRejectedValueOnce(new TypeError('Failed to fetch'))
      .mockResolvedValue(jsonResponse(STARTED))
    vi.stubGlobal('fetch', fetchMock)
    const keys = createAiRunStartKeyring()

    await expect(start(keys, { goal: 'first goal' })).rejects.toBeDefined()
    await expect(start(keys, { goal: 'second goal' })).rejects.toBeDefined()
    await expect(start(keys, { goal: 'second goal' }, 'session_2')).rejects.toBeDefined()

    const sent = [0, 1, 2].map((call) => sentKey(fetchMock, call))
    expect(new Set(sent).size).toBe(3)
  })

  it('treats a goal that differs only by surrounding blanks as the same start', () => {
    const keys = createAiRunStartKeyring()
    const first = keys.keyFor('session_1', { goal: 'find it' })
    expect(keys.keyFor('session_1', { goal: '  find it ' })).toBe(first)
  })

  it('treats a changed limit as a different start (the gateway would refuse the reuse)', () => {
    const keys = createAiRunStartKeyring()
    const first = keys.keyFor('session_1', { goal: 'g', maxSteps: 5 })
    expect(keys.keyFor('session_1', { goal: 'g', maxSteps: 6 })).not.toBe(first)
  })

  it('never reuses an ambiguous unattached start key for an attachment-required run', () => {
    const keys = createAiRunStartKeyring()
    const previous = keys.keyFor('session_1', { goal: 'g' })
    expect(keys.keyFor('session_1', { goal: 'g', requireAttached: true })).not.toBe(previous)
  })
})

describe('a refused AI-run start, described for the person', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  async function refusal(status: number, code: string, headers: Record<string, string> = {}): Promise<unknown> {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        new Response(JSON.stringify({ error: { code, message: 'upstream text that is never shown' } }), {
          status,
          headers: { 'Content-Type': 'application/json', ...headers },
        }),
      ),
    )
    return startBrowserAiRun('org_1', 'session_1', { goal: 'g' }, undefined, { idempotencyKey: 'ai-run-1' }).then(
      () => {
        throw new Error('the start was expected to be refused')
      },
      (thrown: unknown) => thrown,
    )
  }

  const no = (noText: string) => noText
  const en = (_noText: string, enText: string) => enText

  it.each([
    [400, 'invalid_idempotency_key'],
    [409, 'browser_idempotency_key_reused'],
    [409, 'browser_idempotency_key_run_ended'],
  ])('answers %i %s with its own sentence in Norwegian and English, never the gateway text', async (status, code) => {
    const failure = await refusal(status, code)
    const norwegian = describeApiError(failure, no)
    const english = describeApiError(failure, en)
    expect(norwegian).toMatchObject({ code, kind: 'client', message: ERROR_COPY[code]!.no })
    expect(english.message).toBe(ERROR_COPY[code]!.en)
    expect(norwegian.message).not.toBe(english.message)
    expect(`${norwegian.message} ${english.message}`).not.toMatch(/upstream text|idempotency/i)
  })

  it('answers 503 browser_idempotency_unverified with the retry hint the gateway sent', async () => {
    const failure = await refusal(503, 'browser_idempotency_unverified', { 'Retry-After': '5' })
    expect(describeApiError(failure, no)).toMatchObject({
      code: 'browser_idempotency_unverified',
      kind: 'server',
      message: `${ERROR_COPY.browser_idempotency_unverified!.no} Prøv igjen om 5 sekunder.`,
      retryAfterSeconds: 5,
    })
    expect(describeApiError(failure, en).message).toBe(`${ERROR_COPY.browser_idempotency_unverified!.en} Try again in 5 seconds.`)
  })
})

describe('controlBrowserAiRun', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('posts the requested action to the run control route', async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse({ status: 'paused' }))
    vi.stubGlobal('fetch', fetchMock)

    await controlBrowserAiRun('org_1', 'run_1', 'pause')

    const [path, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(path).toContain('/api/v1/browser/runs/run_1/control')
    expect(init.method).toBe('POST')
    expect(JSON.parse(init.body as string)).toEqual({ action: 'pause' })
  })
})
