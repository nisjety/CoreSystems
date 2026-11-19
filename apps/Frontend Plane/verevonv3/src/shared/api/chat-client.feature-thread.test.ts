import { afterEach, describe, expect, it, vi, type Mock } from 'vitest'
import { ReasoningTranslateContractError } from '@/shared/actions/chat-reasoning-translate'
import { ReviewRunContractError } from '@/shared/actions/chat-review'
import { runReview, translateText } from './chat-client'

// The side features' request bodies (W4-BYO, DEC-V-R8): a review and a reasoning translation name the thread the text
// belongs to and the model its connection serves, and send them only when given. A request that names neither is the body
// it was before the feature named a thread.

const REVIEW = {
  target: 'artifact_revision' as const,
  label: 'Tilbud v2',
  material: 'Prisen er 900 kr.',
  brief: '',
  locale: 'no' as const,
}

const REVIEW_CARD = {
  runId: 'run-1',
  target: 'artifact_revision',
  label: 'Tilbud v2',
  status: 'completed',
  verdict: 'clean',
  findings: [],
  sourceChecks: [],
  limitReached: false,
  reviewed: { contentHash: 'hash', lines: 1, bytes: 17 },
  modelUsed: 'gpt-5-codex',
  usage: { inputTokens: 1, outputTokens: 1, costUsd: null, latencyMs: 1 },
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

describe('runReview request body', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('sends the review fields alone when no thread is named, exactly as before', async () => {
    const fetchMock = stubFetch(reply(200, { data: { run: REVIEW_CARD } }))
    // The gateway answers `{ data: { run } }`, and the card is read from the run.
    await expect(runReview(REVIEW)).resolves.toMatchObject({ runId: 'run-1', verdict: 'clean' })
    expect(sentUrl(fetchMock)).toContain('/api/v1/ai/review')
    expect(sentBody(fetchMock)).toBe(
      '{"target":"artifact_revision","label":"Tilbud v2","material":"Prisen er 900 kr.","brief":"","locale":"no"}',
    )
  })

  it('leaves out a thread and a model that are not given, so the body is the review fields alone', async () => {
    const fetchMock = stubFetch(reply(200, { data: { run: REVIEW_CARD } }))
    await runReview({ ...REVIEW, threadId: undefined, model: undefined })
    expect(sentBody(fetchMock)).not.toContain('thread_id')
    expect(sentBody(fetchMock)).not.toContain('"model"')
  })

  it('sends the thread the text belongs to, as thread_id, and the connection model with it', async () => {
    const fetchMock = stubFetch(reply(200, { data: { run: REVIEW_CARD } }))
    await runReview({ ...REVIEW, threadId: 'thread-1', model: 'gpt-5-codex' })
    expect(sentBody(fetchMock)).toBe(
      '{"target":"artifact_revision","label":"Tilbud v2","material":"Prisen er 900 kr.","brief":"","locale":"no","thread_id":"thread-1","model":"gpt-5-codex"}',
    )
  })

  it('sends no model without a thread: a model belongs to the thread its connection serves', async () => {
    const fetchMock = stubFetch(reply(200, { data: { run: REVIEW_CARD } }))
    await runReview({ ...REVIEW, model: 'gpt-5-codex' })
    expect(sentBody(fetchMock)).toBe(
      '{"target":"artifact_revision","label":"Tilbud v2","material":"Prisen er 900 kr.","brief":"","locale":"no"}',
    )
  })

  it('refuses a malformed thread before anything is sent', async () => {
    const fetchMock = stubFetch(reply(200, { data: { run: REVIEW_CARD } }))
    await expect(runReview({ ...REVIEW, threadId: 'thread 1' })).rejects.toThrow(ReviewRunContractError)
    expect(fetchMock).not.toHaveBeenCalled()
  })
})

describe('translateText request body', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('sends the text and the locale alone when no thread is named, exactly as before', async () => {
    const fetchMock = stubFetch(reply(200, { data: { text: 'Hello' } }))
    await expect(translateText('Hei', 'en')).resolves.toBe('Hello')
    expect(sentUrl(fetchMock)).toContain('/api/v1/ai/translate')
    expect(sentBody(fetchMock)).toBe('{"text":"Hei","target_locale":"en"}')
  })

  it('sends the thread and the connection model with the text when they are named', async () => {
    const fetchMock = stubFetch(reply(200, { data: { text: 'Hello' } }))
    await translateText('Hei', 'en', { threadId: 'thread-1', model: 'gpt-5-codex' })
    expect(sentBody(fetchMock)).toBe('{"text":"Hei","target_locale":"en","thread_id":"thread-1","model":"gpt-5-codex"}')
  })

  it('sends the thread alone on a platform thread, with no model', async () => {
    const fetchMock = stubFetch(reply(200, { data: { text: 'Hello' } }))
    await translateText('Hei', 'en', { threadId: 'thread-1', model: undefined })
    expect(sentBody(fetchMock)).toBe('{"text":"Hei","target_locale":"en","thread_id":"thread-1"}')
  })

  it('sends no model without a thread: a model belongs to the thread its connection serves', async () => {
    const fetchMock = stubFetch(reply(200, { data: { text: 'Hello' } }))
    await translateText('Hei', 'en', { model: 'gpt-5-codex' })
    expect(sentBody(fetchMock)).toBe('{"text":"Hei","target_locale":"en"}')
  })

  it('refuses a malformed thread before anything is sent', async () => {
    const fetchMock = stubFetch(reply(200, { data: { text: 'Hello' } }))
    await expect(translateText('Hei', 'en', { threadId: 'thread 1' })).rejects.toThrow(ReasoningTranslateContractError)
    expect(fetchMock).not.toHaveBeenCalled()
  })

  it('rejects a zero-retention refusal with its typed error, so the reader is told why and not that it is unavailable', async () => {
    stubFetch(reply(412, { error: { code: 'zdr_connection_not_acknowledged', message: 'refused' } }))
    await expect(translateText('Hei', 'en', { threadId: 'thread-1', model: 'gpt-5-codex' })).rejects.toMatchObject({
      status: 412,
      code: 'zdr_connection_not_acknowledged',
    })
  })

  it('resolves to null while the switch is off (404), and for a 412 that is not a zero-retention refusal', async () => {
    stubFetch(reply(404, { error: { code: 'reasoning_translate_disabled', message: 'off' } }))
    await expect(translateText('Hei', 'en')).resolves.toBeNull()
    stubFetch(reply(412, { error: { code: 'reasoning_translate_unavailable', message: 'x' } }))
    await expect(translateText('Hei', 'en')).resolves.toBeNull()
  })
})
