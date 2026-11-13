import { afterEach, describe, expect, it, vi, type Mock } from 'vitest'
import { ApiError } from './api-error'
import { askSideChat } from './chat-client'

// The side question's client (F-t3-069, W4-BYO and DEC-V-R8). The body carries the question, and the model only when the
// thread is a connection thread names one: a platform thread's body is unchanged. A refusal reaches the caller as the
// server's typed ApiError, so the panel can tell a permanent refusal from a transient one by its code.

const ANSWER = {
  answer: 'Det er et svar.',
  model_used: 'gpt-5-codex',
  usage: { input_tokens: 12, output_tokens: 7, cost_usd: null },
}

function reply(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } })
}

type FetchMock = Mock<(input: RequestInfo | URL, init?: RequestInit) => Promise<Response>>

/** Stubs fetch with one answer and returns the spy, so a test can read the URL and the body that were sent. */
function stubFetch(response: Response): FetchMock {
  const fetchMock = vi.fn<(input: RequestInfo | URL, init?: RequestInit) => Promise<Response>>(async () => response.clone())
  vi.stubGlobal('fetch', fetchMock)
  return fetchMock
}

function sentBody(fetchMock: FetchMock): string {
  return String(fetchMock.mock.calls[0]?.[1]?.body)
}

function sentUrl(fetchMock: FetchMock): string {
  return String(fetchMock.mock.calls[0]?.[0])
}

describe('askSideChat request', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('sends the question alone on a platform thread, exactly as before', async () => {
    const fetchMock = stubFetch(reply(200, ANSWER))
    const result = await askSideChat('thread-1', 'Hva ble sagt?')
    expect(sentBody(fetchMock)).toBe('{"question":"Hva ble sagt?"}')
    expect(sentUrl(fetchMock)).toContain('/api/v1/chat/threads/thread-1/side-chat')
    expect(fetchMock.mock.calls[0]?.[1]?.method).toBe('POST')
    expect(result).toEqual({
      answer: 'Det er et svar.',
      modelUsed: 'gpt-5-codex',
      usage: { inputTokens: 12, outputTokens: 7, costUsd: null },
    })
  })

  it('sends the connection thread model with the question when one is named', async () => {
    const fetchMock = stubFetch(reply(200, ANSWER))
    await askSideChat('thread-1', 'Hva ble sagt?', 'gpt-5-codex')
    expect(sentBody(fetchMock)).toBe('{"question":"Hva ble sagt?","model":"gpt-5-codex"}')
  })

  it('sends the model trimmed, as the contract reads it', async () => {
    const fetchMock = stubFetch(reply(200, ANSWER))
    await askSideChat('thread-1', 'Hei', '  gpt-5-codex ')
    expect(sentBody(fetchMock)).toBe('{"question":"Hei","model":"gpt-5-codex"}')
  })

  it('treats a blank model as no model, so the body is the question alone', async () => {
    const fetchMock = stubFetch(reply(200, ANSWER))
    await askSideChat('thread-1', 'Hei', '   ')
    expect(sentBody(fetchMock)).toBe('{"question":"Hei"}')
  })

  it('refuses a question the contract refuses before anything is sent', async () => {
    const fetchMock = stubFetch(reply(200, ANSWER))
    await expect(askSideChat('thread-1', '   ')).rejects.toThrow()
    await expect(askSideChat('thread-1', 'Hei', 'gpt 5')).rejects.toThrow()
    expect(fetchMock).not.toHaveBeenCalled()
  })

  it('resolves to null while the side chat is switched off', async () => {
    stubFetch(reply(404, { error: { code: 'side_chat_disabled', message: 'off' } }))
    await expect(askSideChat('thread-1', 'Hei', 'gpt-5-codex')).resolves.toBeNull()
  })
})

describe('askSideChat refusals', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it.each([
    [403, 'model_not_entitled'],
    [400, 'side_chat_model_required'],
    [409, 'connection_ambiguous'],
    [409, 'side_chat_thread_retention_refused'],
    [409, 'scope_not_available_in_space'],
    [403, 'zdr_posture_mismatch'],
    [503, 'delegated_auth_unavailable'],
  ])('rejects a %i %s with the typed ApiError, so the caller can branch on its code', async (status, code) => {
    stubFetch(reply(status, { error: { code, message: 'English ops text' } }))
    const failure = await askSideChat('thread-1', 'Hei', 'gpt-5-codex').then(
      () => null,
      (error: unknown) => error,
    )
    expect(failure).toBeInstanceOf(ApiError)
    expect(failure).toMatchObject({ status, code })
  })

  it.each([
    [409, 'subscription_reauth_required'],
    [429, 'subscription_usage_limit'],
    [422, 'subscription_bad_request'],
    [413, 'prompt_too_long'],
  ])(
    'reads the top-level code of a ChatGPT subscription refusal, %i %s, the shape Model Gateway answers it in',
    async (status, code) => {
      stubFetch(reply(status, { error: 'English ops text', code, retryable: false }))
      const failure = await askSideChat('thread-1', 'Hei', 'gpt-5-codex').catch((error: unknown) => error)
      expect(failure).toBeInstanceOf(ApiError)
      expect(failure).toMatchObject({ status, code, retryAfterSeconds: null })
    },
  )

  it("keeps a model_not_entitled refusal's typed details, so its reason can be read", async () => {
    stubFetch(
      reply(403, {
        error: { code: 'model_not_entitled', message: 'm', details: { reason: 'explicitly_denied', model: 'gpt-5-codex' } },
      }),
    )
    const failure = await askSideChat('thread-1', 'Hei', 'gpt-5-codex').catch((error: unknown) => error)
    expect(failure).toBeInstanceOf(ApiError)
    expect((failure as ApiError).details).toEqual({ reason: 'explicitly_denied', model: 'gpt-5-codex' })
  })
})
