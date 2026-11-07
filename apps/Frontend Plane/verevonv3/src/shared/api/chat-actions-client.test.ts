import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  BUILTIN_ACTIONS,
  LEARN_ACTION,
  learnCommandAvailable,
  requestLearnFromThread,
  type LearnCommandContext,
} from './chat-actions-client'

function jsonResponse(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    status,
    headers: { 'Content-Type': 'application/json' },
  })
}

afterEach(() => {
  vi.unstubAllGlobals()
})

const saved: LearnCommandContext = {
  threadId: 'thread-1',
  temporaryChat: false,
  zeroRetention: false,
  learningAllowed: true,
}

describe('/learn (EP-11 S7)', () => {
  it('is not a fixed built-in, so a composer that does not register it is unchanged', () => {
    expect(BUILTIN_ACTIONS.map((action) => action.id)).not.toContain(LEARN_ACTION.id)
    expect(LEARN_ACTION).toMatchObject({ id: '__learn', kind: 'builtin', builtin: 'learn' })
  })

  it('learn_hidden_on_temporary_chat', () => {
    expect(learnCommandAvailable(saved)).toBe(true)
    expect(learnCommandAvailable({ ...saved, temporaryChat: true })).toBe(false)
  })

  it('is hidden under zero retention, before the first save, and while learning is off', () => {
    expect(learnCommandAvailable({ ...saved, zeroRetention: true })).toBe(false)
    expect(learnCommandAvailable({ ...saved, threadId: null })).toBe(false)
    expect(learnCommandAvailable({ ...saved, threadId: '  ' })).toBe(false)
    expect(learnCommandAvailable({ ...saved, learningAllowed: false })).toBe(false)
  })

  it('posts the thread and the trimmed note, and returns the queued state', async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse({ data: { state: 'queued' } }, 202))
    vi.stubGlobal('fetch', fetchMock)

    await expect(requestLearnFromThread('thread-1', '  keep the steps  ')).resolves.toEqual({ state: 'queued' })

    const [path, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(path).toBe('/api/v1/learning/learn')
    expect(init.method).toBe('POST')
    expect(JSON.parse(init.body as string)).toEqual({ threadId: 'thread-1', note: 'keep the steps' })
  })

  it('sends no note when none was written', async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse({ data: { state: 'queued' } }, 202))
    vi.stubGlobal('fetch', fetchMock)

    await requestLearnFromThread('thread-1', '   ')

    const [, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(JSON.parse(init.body as string)).toEqual({ threadId: 'thread-1' })
  })

  it('rejects with the typed zero-retention refusal', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        jsonResponse(
          {
            error: {
              code: 'zdr_feature_not_supported',
              message: 'This is not supported with zero data retention yet: learning from this conversation.',
              retryable: false,
              action: 'turn_off_feature',
              feature: 'learn_command',
            },
          },
          412,
        ),
      ),
    )

    await expect(requestLearnFromThread('thread-1')).rejects.toBeTruthy()
  })
})
