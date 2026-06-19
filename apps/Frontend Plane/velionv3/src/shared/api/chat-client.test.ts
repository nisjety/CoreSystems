import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  buildChatWireBody,
  getChatThreadTranscript,
  listChatThreads,
  saveChatThreadSnapshot,
  streamChat,
} from './chat-client'

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

  it('does not request the tools SSE family or web_search by default', () => {
    const body = buildChatWireBody({ content: 'hi' })

    expect(body.features).toEqual(expect.not.arrayContaining(['tools']))
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
