import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  buildChatWireBody,
  describeFeedbackFailure,
  getChatThreadTranscript,
  listChatThreads,
  saveChatThreadSnapshot,
  streamChat,
  submitFeedback,
} from './chat-client'
import { ApiError } from './http'

function sseResponse(frames: string[]): Response {
  const encoder = new TextEncoder()
  const stream = new ReadableStream<Uint8Array>({
    start(controller) {
      for (const frame of frames) controller.enqueue(encoder.encode(frame))
      controller.close()
    },
  })
  return new Response(stream, {
    status: 200,
    headers: { 'Content-Type': 'text/event-stream' },
  })
}

describe('chat-client connected event', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('maps the agentic run_id into ChatConnectedEvent.runId', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      sseResponse([
        `event: connected\ndata: ${JSON.stringify({
          ok: true,
          request_id: 'req_1',
          thread_id: 'thread_1',
          model: 'gpt-4o',
          run_id: 'run_1',
        })}\n\n`,
      ]),
    )
    vi.stubGlobal('fetch', fetchMock)

    const onConnected = vi.fn()
    await streamChat({ content: 'hi' }, { onConnected })

    expect(onConnected).toHaveBeenCalledWith(
      expect.objectContaining({ ok: true, requestId: 'req_1', threadId: 'thread_1', runId: 'run_1' }),
    )
  })
})

describe('chat-client tool wiring', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('requests the tools family by default (so knowledge_search/fetch_url attach server-side) but declares no client tools, and never web_search', () => {
    const body = buildChatWireBody({ content: 'hi' })

    // model-gateway merges its own builtins (knowledge_search, fetch_url) into
    // the tool list whenever 'tools' is present, regardless of the client's
    // own (here empty) tools array — see buildChatWireBody's comment.
    expect(body.features).toEqual(expect.arrayContaining(['tools']))
    expect(body.tools).toEqual([])
  })

  it('requests web_search only when browsing is enabled', () => {
    const body = buildChatWireBody({ content: 'latest news', browseWeb: true })

    expect(body.browse_web).toBe(true)
    expect(body.features).toEqual(expect.arrayContaining(['tools']))
    expect(body.tools).toEqual([
      expect.objectContaining({ name: 'web_search' }),
    ])
  })

  it('maps batched citation events into individual citations', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      sseResponse([
        `event: citations\ndata: ${JSON.stringify({
          citations: [
            { id: 'c1', title: 'Result', url: 'https://example.com', snippet: 'Snippet' },
          ],
        })}\n\n`,
      ]),
    )
    vi.stubGlobal('fetch', fetchMock)

    const onCitation = vi.fn()
    await streamChat({ content: 'hi' }, { onCitation })

    expect(onCitation).toHaveBeenCalledWith({
      id: 'c1',
      title: 'Result',
      url: 'https://example.com',
      snippet: 'Snippet',
    })
  })

  it('maps a follow_ups event into onFollowUps, dropping non-string/empty entries', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      sseResponse([
        `event: follow_ups\ndata: ${JSON.stringify({
          suggestions: ['Hva med frakt til Bergen?', '', 42, 'Kan jeg spore ordren?'],
          request_id: 'req_1',
        })}\n\n`,
      ]),
    )
    vi.stubGlobal('fetch', fetchMock)

    const onFollowUps = vi.fn()
    await streamChat({ content: 'hi' }, { onFollowUps })

    expect(onFollowUps).toHaveBeenCalledWith({
      suggestions: ['Hva med frakt til Bergen?', 'Kan jeg spore ordren?'],
      requestId: 'req_1',
    })
  })

  it('never calls onFollowUps when the event carries no usable suggestions', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      sseResponse([
        `event: follow_ups\ndata: ${JSON.stringify({ suggestions: [] })}\n\n`,
      ]),
    )
    vi.stubGlobal('fetch', fetchMock)

    const onFollowUps = vi.fn()
    await streamChat({ content: 'hi' }, { onFollowUps })

    expect(onFollowUps).not.toHaveBeenCalled()
  })
})

describe('chat-client server thread history', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('lists server-backed chat thread sessions', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(
      new Response(JSON.stringify({
        data: {
          sessions: [
            {
              threadId: 'thread_1',
              title: 'Server saved chat',
              preview: 'Last answer',
              updatedAt: '2026-06-17T10:00:00.000Z',
            },
          ],
        },
      }), { headers: { 'Content-Type': 'application/json' }, status: 200 }),
    ))

    await expect(listChatThreads()).resolves.toEqual([
      {
        threadId: 'thread_1',
        title: 'Server saved chat',
        preview: 'Last answer',
        updatedAt: '2026-06-17T10:00:00.000Z',
      },
    ])
  })

  it('saves a transcript snapshot to the thread endpoint', async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      new Response(JSON.stringify({
        data: {
          session: {
            threadId: 'thread_1',
            title: 'Question',
            preview: 'Answer',
            updatedAt: '2026-06-17T10:00:00.000Z',
          },
        },
      }), { headers: { 'Content-Type': 'application/json' }, status: 200 }),
    )
    vi.stubGlobal('fetch', fetchMock)

    await saveChatThreadSnapshot('thread_1', {
      title: 'Question',
      preview: 'Answer',
      updatedAt: '2026-06-17T10:00:00.000Z',
      turns: [{ id: 'user_1', role: 'user', content: 'Question' }],
      taskSteps: [],
    })

    expect(fetchMock).toHaveBeenCalledWith('/api/v1/chat/threads/thread_1', expect.objectContaining({
      method: 'PUT',
      credentials: 'include',
    }))
    const firstCall = fetchMock.mock.calls[0]
    expect(firstCall).toBeTruthy()
    const init = firstCall?.[1]
    expect(JSON.parse(String(init?.body))).toMatchObject({
      title: 'Question',
      turns: [{ id: 'user_1', role: 'user', content: 'Question' }],
    })
  })

  it('loads a rich server transcript when one exists', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(
      new Response(JSON.stringify({
        data: {
          transcript: {
            threadId: 'thread_1',
            updatedAt: '2026-06-17T10:00:00.000Z',
            turns: [{ id: 'asst_1', role: 'assistant', content: 'Answer' }],
            taskSteps: [{ id: 'step_1', title: 'Search', detail: 'Done', status: 'done' }],
          },
        },
      }), { headers: { 'Content-Type': 'application/json' }, status: 200 }),
    ))

    await expect(getChatThreadTranscript('thread_1')).resolves.toMatchObject({
      threadId: 'thread_1',
      turns: [{ id: 'asst_1', role: 'assistant', content: 'Answer' }],
      taskSteps: [{ id: 'step_1', title: 'Search' }],
    })
  })
})

describe('chat feedback', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  function jsonResponse(body: unknown, status: number): Response {
    return new Response(JSON.stringify(body), {
      status,
      headers: { 'Content-Type': 'application/json' },
    })
  }

  it('posts the thumb polarity and never asserts a skill id', async () => {
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse({ accepted: true }, 200))
    vi.stubGlobal('fetch', fetchMock)

    await submitFeedback('req_1', 'positive', { note: 'nyttig', runId: 'run_1' })

    expect(fetchMock).toHaveBeenCalledWith(
      '/api/v1/chat/feedback',
      expect.objectContaining({ method: 'POST' }),
    )
    const body = JSON.parse(String(fetchMock.mock.calls[0]?.[1]?.body))
    expect(body).toMatchObject({ requestId: 'req_1', rating: 'positive', note: 'nyttig', run_id: 'run_1' })
    // Which skills steered the answer is resolved server-side; a client-asserted
    // skill id would let a rating be aimed at any skill in the org.
    expect(body.skill_id).toBeUndefined()
    expect(body.skillId).toBeUndefined()
  })

  it('rejects rather than resolving when the rating was not recorded', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(
      jsonResponse({ error: { code: 'turn_not_rateable', message: 'gone' } }, 409),
    ))

    // A resolved promise here is the reported defect: the UI lights the thumb
    // while nothing persisted.
    await expect(submitFeedback('req_1', 'negative')).rejects.toBeInstanceOf(ApiError)
  })

  it('describes each failure so the surface can show it quietly', () => {
    expect(describeFeedbackFailure(new ApiError('gone', 409, 'turn_not_rateable')))
      .toContain('ikke vurderes lenger')
    expect(describeFeedbackFailure(new ApiError('denied', 403, null)))
      .toContain('ikke tilgang')
    expect(describeFeedbackFailure(new ApiError('zdr', 412, null)))
      .toContain('Zero Data Retention')
    // Anything unrecognised still says the rating did not stick.
    expect(describeFeedbackFailure(new Error('boom'))).toContain('Kunne ikke lagre')
  })
})
