import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type { ChatStreamHandlers } from '@/shared/api/chat-client'

type StreamCall = { request: Record<string, unknown>; handlers: ChatStreamHandlers; signal: AbortSignal }

const streamChat = vi.fn()
const resumeStream = vi.fn()
const cancelInvocation = vi.fn()
const updateSpaceThreadPresentation = vi.fn()

vi.mock('@/shared/api/chat-client', () => ({
  streamChat: (...args: unknown[]) => streamChat(...args),
  resumeStream: (...args: unknown[]) => resumeStream(...args),
  cancelInvocation: (...args: unknown[]) => cancelInvocation(...args),
}))

vi.mock('@/shared/api/spaces-client', () => ({
  updateSpaceThreadPresentation: (...args: unknown[]) => updateSpaceThreadPresentation(...args),
  createSpacePost: vi.fn(),
  replyInSpaceThread: vi.fn(),
}))

import { createSpaceSendPipeline, newSpaceSendId, spaceSendSessionKey, type SpaceSendInput, type SpaceSendStage } from './space-send-pipeline'
import { noteSendTransportOk, spaceConnectivity } from './space-connectivity'

// A neutral test selection: no live model is ever called, the transport is mocked.
const model = { model: 'test-model', label: 'Test model' } as const

function input(overrides: Partial<SpaceSendInput> = {}): SpaceSendInput {
  return { content: 'hello', mode: 'ask', model, skills: [], ...overrides }
}

function pipelineWith(options: { sendIdempotency?: boolean; stages?: SpaceSendStage[] } = {}) {
  const host = {
    spaceRef: 'room',
    onExchangeSettled: vi.fn(),
    onClearReplyTarget: vi.fn(),
    onArtifact: vi.fn(),
  }
  const stages = options.stages ?? []
  const pipeline = createSpaceSendPipeline({
    host,
    resolveTier: async () => ({ tier: undefined, source: 'intent-mode' }),
    describeError: (event) => `described:${event.code ?? 'unknown'}`,
    onStage: (stage) => stages.push(stage),
    sendIdempotency: () => options.sendIdempotency ?? false,
  })
  return { pipeline, host, stages }
}

/** A stream that stays open until the caller aborts it, then rejects the way fetch does. */
function openStream(): Promise<StreamCall> {
  return new Promise((resolveCall) => {
    streamChat.mockImplementationOnce((request: Record<string, unknown>, handlers: ChatStreamHandlers, signal: AbortSignal) => {
      resolveCall({ request, handlers, signal })
      return new Promise<void>((_resolve, reject) => {
        signal.addEventListener('abort', () => reject(new DOMException('aborted', 'AbortError')))
      })
    })
  })
}

beforeEach(() => {
  streamChat.mockReset()
  resumeStream.mockReset()
  cancelInvocation.mockReset()
  updateSpaceThreadPresentation.mockReset()
  updateSpaceThreadPresentation.mockResolvedValue(undefined)
  noteSendTransportOk()
})

afterEach(() => {
  vi.useRealTimers()
})

describe('space send ids', () => {
  it('mints a 32 hex character id that fits the client_send_id shape', () => {
    const id = newSpaceSendId()
    expect(id).toMatch(/^[0-9a-f]{32}$/)
    expect(id).toMatch(/^[A-Za-z0-9_-]{16,64}$/)
    expect(newSpaceSendId()).not.toBe(id)
  })

  it('names the session key of a new-thread send from its id', () => {
    expect(spaceSendSessionKey('abc')).toBe('space-post-abc')
  })
})

describe('space send pipeline (P-SPA2)', () => {
  describe('stages', () => {
    it('emits started, accepted on connected, settled in order', async () => {
      const { pipeline, stages } = pipelineWith()
      streamChat.mockImplementationOnce(async (_request: unknown, handlers: ChatStreamHandlers) => {
        handlers.onConnected?.({ requestId: 'req_1', threadId: 'thr_1' } as never)
        handlers.onMessage?.({ content: 'Hi' } as never)
        handlers.onDone?.({} as never)
      })
      await pipeline.send(input())
      flush()
      expect(stages).toEqual(['started', 'accepted', 'settled'])
      expect(pipeline.exchange()).toMatchObject({ userContent: 'hello', assistantContent: 'Hi', status: 'done' })
      expect(pipeline.submitting()).toBe(false)
    })

    it('reports accepted once even if a resumed stream announces itself again', async () => {
      const { pipeline, stages } = pipelineWith()
      streamChat.mockImplementationOnce(async (_request: unknown, handlers: ChatStreamHandlers) => {
        handlers.onConnected?.({ requestId: 'req_1' } as never)
        handlers.onConnected?.({ requestId: 'req_1' } as never)
        handlers.onDone?.({} as never)
      })
      await pipeline.send(input())
      expect(stages.filter((stage) => stage === 'accepted')).toHaveLength(1)
    })

    it('still settles when the transport rejects', async () => {
      const { pipeline, stages } = pipelineWith()
      streamChat.mockRejectedValueOnce(new Error('boom'))
      await pipeline.send(input())
      flush()
      expect(stages).toEqual(['started', 'settled'])
      expect(pipeline.exchange()).toMatchObject({ status: 'error', errorMessage: 'boom' })
    })
  })

  describe('request shape', () => {
    it('streams the ask mode with the room, the model and the picked skills as actions', async () => {
      const { pipeline } = pipelineWith()
      streamChat.mockResolvedValueOnce(undefined)
      await pipeline.send(
        input({
          mentionedAgentRef: 'agent_1',
          skills: [{ id: 'skill_1', name: 'Summarise' } as never],
        }),
      )
      const request = streamChat.mock.calls[0]?.[0] as Record<string, unknown>
      expect(request).toMatchObject({
        content: 'hello',
        model: 'test-model',
        spaceRef: 'room',
        mentionedAgentRef: 'agent_1',
        threadId: undefined,
        actions: [{ id: 'skill_1', name: 'Summarise', kind: 'skill' }],
      })
      // Idempotency off: neither key is on the wire.
      expect(request.clientMessageId).toBeUndefined()
      expect(request.sessionKey).toBeUndefined()
    })

    it('with the send key on, a new thread carries the send id and a session key', async () => {
      const { pipeline } = pipelineWith({ sendIdempotency: true })
      streamChat.mockResolvedValueOnce(undefined)
      await pipeline.send(input())
      const request = streamChat.mock.calls[0]?.[0] as Record<string, unknown>
      expect(request.clientMessageId).toMatch(/^[0-9a-f]{32}$/)
      expect(request.sessionKey).toBe(`space-post-${String(request.clientMessageId)}`)
    })

    it('with the send key on, a reply keeps the thread and has no session key', async () => {
      const { pipeline } = pipelineWith({ sendIdempotency: true })
      streamChat.mockResolvedValueOnce(undefined)
      await pipeline.send(input({ replyThreadId: 'thr_7' }))
      const request = streamChat.mock.calls[0]?.[0] as Record<string, unknown>
      expect(request.threadId).toBe('thr_7')
      expect(request.clientMessageId).toBeTruthy()
      expect(request.sessionKey).toBeUndefined()
    })

    it('uses the id a retry passes back instead of minting a new one', async () => {
      const { pipeline } = pipelineWith({ sendIdempotency: true })
      streamChat.mockResolvedValueOnce(undefined)
      await pipeline.send(input({ sendId: 'f'.repeat(32) }))
      expect((streamChat.mock.calls[0]?.[0] as Record<string, unknown>).clientMessageId).toBe('f'.repeat(32))
    })
  })

  describe('stop', () => {
    it('keeps a stop that landed when a done follows it', async () => {
      const { pipeline } = pipelineWith()
      streamChat.mockImplementationOnce(async (_request: unknown, handlers: ChatStreamHandlers) => {
        handlers.onConnected?.({ requestId: 'req_1' } as never)
        handlers.onStopped?.({} as never)
        handlers.onDone?.({} as never)
      })
      await pipeline.send(input())
      flush()
      expect(pipeline.exchange()).toMatchObject({ status: 'stopped', stopRecorded: true })
    })

    it('cancels by requestId and records the stop', async () => {
      const { pipeline, host } = pipelineWith()
      const opened = openStream()
      cancelInvocation.mockResolvedValue(undefined)
      const sending = pipeline.send(input())
      const call = await opened
      call.handlers.onConnected?.({ requestId: 'req_9', threadId: 'thr_1' } as never)
      await pipeline.stop()
      await sending
      flush()
      expect(call.signal.aborted).toBe(true)
      expect(cancelInvocation).toHaveBeenCalledWith('req_9')
      expect(pipeline.exchange()).toMatchObject({ status: 'stopped', stopRecorded: true })
      expect(host.onExchangeSettled).toHaveBeenCalled()
    })

    it('leaves stopRecorded false when no request id was ever announced', async () => {
      const { pipeline } = pipelineWith()
      const opened = openStream()
      const sending = pipeline.send(input())
      await opened
      await pipeline.stop()
      await sending
      flush()
      expect(cancelInvocation).not.toHaveBeenCalled()
      expect(pipeline.exchange()).toMatchObject({ status: 'stopped', stopRecorded: false })
    })

    it('leaves stopRecorded false when the cancel call fails', async () => {
      const { pipeline } = pipelineWith()
      const opened = openStream()
      cancelInvocation.mockRejectedValue(new Error('offline'))
      const sending = pipeline.send(input())
      const call = await opened
      call.handlers.onConnected?.({ requestId: 'req_9' } as never)
      await pipeline.stop()
      await sending
      flush()
      expect(pipeline.exchange()).toMatchObject({ status: 'stopped', stopRecorded: false })
    })

    it('does not treat an abort we caused as an error', async () => {
      const { pipeline } = pipelineWith()
      const opened = openStream()
      const sending = pipeline.send(input())
      await opened
      await pipeline.stop()
      await sending
      flush()
      expect(pipeline.exchange()?.status).toBe('stopped')
      expect(pipeline.exchange()?.errorMessage).toBeUndefined()
    })

    it('does nothing when no send is in flight', async () => {
      const { pipeline } = pipelineWith()
      await pipeline.stop()
      expect(cancelInvocation).not.toHaveBeenCalled()
      expect(pipeline.exchange()).toBeUndefined()
    })
  })

  describe('title write-back', () => {
    it('writes a generated title back only for a thread this browser opened', async () => {
      const { pipeline, host } = pipelineWith()
      streamChat.mockImplementationOnce(async (_request: unknown, handlers: ChatStreamHandlers) => {
        handlers.onConnected?.({ requestId: 'req_1', threadId: 'thr_new' } as never)
        handlers.onTitle?.({ title: '  A name  ' } as never)
        handlers.onDone?.({} as never)
      })
      await pipeline.send(input())
      await vi.waitFor(() => expect(updateSpaceThreadPresentation).toHaveBeenCalledTimes(1))
      expect(updateSpaceThreadPresentation).toHaveBeenCalledWith('room', 'thr_new', { title: 'A name' })
      await vi.waitFor(() => expect(host.onExchangeSettled).toHaveBeenCalled())
    })

    it('never writes a title for a reply into an existing thread', async () => {
      const { pipeline } = pipelineWith()
      streamChat.mockImplementationOnce(async (_request: unknown, handlers: ChatStreamHandlers) => {
        handlers.onConnected?.({ requestId: 'req_1', threadId: 'thr_7' } as never)
        handlers.onTitle?.({ title: 'A name' } as never)
        handlers.onDone?.({} as never)
      })
      await pipeline.send(input({ replyThreadId: 'thr_7' }))
      expect(updateSpaceThreadPresentation).not.toHaveBeenCalled()
    })

    it('ignores a blank title', async () => {
      const { pipeline } = pipelineWith()
      streamChat.mockImplementationOnce(async (_request: unknown, handlers: ChatStreamHandlers) => {
        handlers.onConnected?.({ requestId: 'req_1', threadId: 'thr_new' } as never)
        handlers.onTitle?.({ title: '   ' } as never)
        handlers.onDone?.({} as never)
      })
      await pipeline.send(input())
      expect(updateSpaceThreadPresentation).not.toHaveBeenCalled()
    })
  })

  describe('errors', () => {
    it('shows a server error with its code and the room\'s own words', async () => {
      const { pipeline } = pipelineWith()
      streamChat.mockImplementationOnce(async (_request: unknown, handlers: ChatStreamHandlers) => {
        handlers.onError?.({ code: 'spend_hard_stop', message: 'x' } as never)
      })
      await pipeline.send(input())
      flush()
      expect(pipeline.exchange()).toMatchObject({
        status: 'error',
        errorCode: 'spend_hard_stop',
        errorMessage: 'described:spend_hard_stop',
      })
      expect(pipeline.exchange()?.retryable).toBeUndefined()
    })

    it('treats managed_run_already_started as acceptance, not an error', async () => {
      const { pipeline, host } = pipelineWith({ sendIdempotency: true })
      streamChat.mockImplementationOnce(async (_request: unknown, handlers: ChatStreamHandlers) => {
        handlers.onError?.({ code: 'managed_run_already_started', message: 'x' } as never)
      })
      await pipeline.send(input())
      flush()
      expect(pipeline.exchange()).toMatchObject({ status: 'done', acceptedEarlier: true })
      expect(host.onExchangeSettled).toHaveBeenCalled()
    })

    it('marks a connection error before the turn was announced retryable, and the room unreachable', async () => {
      const { pipeline } = pipelineWith({ sendIdempotency: true })
      streamChat.mockImplementationOnce(async (_request: unknown, handlers: ChatStreamHandlers) => {
        handlers.onError?.({ code: 'connection_error', message: 'x' } as never)
      })
      await pipeline.send(input())
      flush()
      expect(pipeline.exchange()).toMatchObject({ status: 'error', retryable: true })
      expect(spaceConnectivity()).toBe('unreachable')
    })

    it('retry repeats the same send under its own id, and only when retryable', async () => {
      const { pipeline } = pipelineWith({ sendIdempotency: true })
      streamChat
        .mockImplementationOnce(async (_request: unknown, handlers: ChatStreamHandlers) => {
          handlers.onError?.({ code: 'connection_error', message: 'x' } as never)
        })
        .mockResolvedValueOnce(undefined)
      await pipeline.send(input())
      flush()
      const firstId = (streamChat.mock.calls[0]?.[0] as Record<string, unknown>).clientMessageId
      await pipeline.retry()
      expect(streamChat).toHaveBeenCalledTimes(2)
      expect((streamChat.mock.calls[1]?.[0] as Record<string, unknown>).clientMessageId).toBe(firstId)
    })

    it('retry does nothing for a non-retryable exchange', async () => {
      const { pipeline } = pipelineWith({ sendIdempotency: true })
      streamChat.mockImplementationOnce(async (_request: unknown, handlers: ChatStreamHandlers) => {
        handlers.onError?.({ code: 'spend_hard_stop', message: 'x' } as never)
      })
      await pipeline.send(input())
      flush()
      await pipeline.retry()
      expect(streamChat).toHaveBeenCalledTimes(1)
    })
  })

  describe('resuming a dropped stream', () => {
    it('resumes by request id after the server announced the turn, then settles done', async () => {
      vi.useFakeTimers()
      const { pipeline } = pipelineWith({ sendIdempotency: true })
      streamChat.mockImplementationOnce(async (_request: unknown, handlers: ChatStreamHandlers) => {
        handlers.onConnected?.({ requestId: 'req_5' } as never)
        handlers.onFrameId?.('4')
        handlers.onError?.({ code: 'connection_error', message: 'x' } as never)
      })
      resumeStream.mockImplementationOnce(async (_requestId: string, handlers: ChatStreamHandlers) => {
        handlers.onMessage?.({ content: 'rest' } as never)
        handlers.onDone?.({} as never)
      })
      const sending = pipeline.send(input())
      await vi.advanceTimersByTimeAsync(1500)
      await sending
      flush()
      expect(resumeStream).toHaveBeenCalledTimes(1)
      expect(resumeStream.mock.calls[0]?.[0]).toBe('req_5')
      expect(resumeStream.mock.calls[0]?.[3]).toBe('4')
      expect(pipeline.exchange()).toMatchObject({ status: 'done', assistantContent: 'rest' })
    })

    it('drops a frame it already applied', async () => {
      const { pipeline } = pipelineWith({ sendIdempotency: true })
      streamChat.mockImplementationOnce(async (_request: unknown, handlers: ChatStreamHandlers) => {
        expect(handlers.acceptFrame?.('3')).toBe(true)
        expect(handlers.acceptFrame?.('3')).toBe(false)
        expect(handlers.acceptFrame?.('2')).toBe(false)
        expect(handlers.acceptFrame?.('4')).toBe(true)
        // A frame with no numeric id is always accepted.
        expect(handlers.acceptFrame?.(undefined)).toBe(true)
        expect(handlers.acceptFrame?.('x')).toBe(true)
        handlers.onDone?.({} as never)
      })
      await pipeline.send(input())
    })

    // WS01-S8: the cursor is the last frame id seen; without one the whole answer is replayed, so the text restarts.
    it('restarts the answer text when the resume has no cursor to continue from', async () => {
      vi.useFakeTimers()
      const { pipeline } = pipelineWith({ sendIdempotency: true })
      streamChat.mockImplementationOnce(async (_request: unknown, handlers: ChatStreamHandlers) => {
        handlers.onConnected?.({ requestId: 'req_5' } as never)
        handlers.onMessage?.({ content: 'Hal' } as never)
        handlers.onError?.({ code: 'connection_error', message: 'x' } as never)
      })
      resumeStream.mockImplementationOnce(async (_requestId: string, handlers: ChatStreamHandlers) => {
        handlers.onMessage?.({ content: 'Hallo igjen' } as never)
        handlers.onDone?.({} as never)
      })
      const sending = pipeline.send(input())
      await vi.advanceTimersByTimeAsync(1500)
      await sending
      flush()
      expect(resumeStream.mock.calls[0]?.[3]).toBeUndefined()
      // Not "HalHallo igjen": the replay starts the text over.
      expect(pipeline.exchange()).toMatchObject({ status: 'done', assistantContent: 'Hallo igjen' })
    })

    it('keeps the text it has when it resumes from a cursor', async () => {
      vi.useFakeTimers()
      const { pipeline } = pipelineWith({ sendIdempotency: true })
      streamChat.mockImplementationOnce(async (_request: unknown, handlers: ChatStreamHandlers) => {
        handlers.onConnected?.({ requestId: 'req_5' } as never)
        handlers.onMessage?.({ content: 'Hal' } as never)
        handlers.onFrameId?.('7')
        handlers.onError?.({ code: 'connection_error', message: 'x' } as never)
      })
      resumeStream.mockImplementationOnce(async (_requestId: string, handlers: ChatStreamHandlers) => {
        handlers.onMessage?.({ content: 'lo' } as never)
        handlers.onDone?.({} as never)
      })
      const sending = pipeline.send(input())
      await vi.advanceTimersByTimeAsync(1500)
      await sending
      flush()
      expect(pipeline.exchange()).toMatchObject({ status: 'done', assistantContent: 'Hallo' })
    })

    it('resumes at most three times with growing waits, then ends in a retryable error and marks the room unreachable', async () => {
      vi.useFakeTimers()
      const { pipeline } = pipelineWith({ sendIdempotency: true })
      streamChat.mockImplementationOnce(async (_request: unknown, handlers: ChatStreamHandlers) => {
        handlers.onConnected?.({ requestId: 'req_5' } as never)
        handlers.onError?.({ code: 'connection_error', message: 'x' } as never)
      })
      resumeStream.mockImplementation(async (_requestId: unknown, handlers: ChatStreamHandlers) => {
        handlers.onError?.({ code: 'connection_error', message: 'x' } as never)
      })
      const sending = pipeline.send(input())

      // The first wait is 1 s: nothing happens at 900 ms.
      await vi.advanceTimersByTimeAsync(900)
      expect(resumeStream).toHaveBeenCalledTimes(0)
      await vi.advanceTimersByTimeAsync(200)
      expect(resumeStream).toHaveBeenCalledTimes(1)
      // The second wait is 2 s, counted from the end of the first resume (t = 1.1 s now).
      await vi.advanceTimersByTimeAsync(1800)
      expect(resumeStream).toHaveBeenCalledTimes(1)
      await vi.advanceTimersByTimeAsync(300)
      expect(resumeStream).toHaveBeenCalledTimes(2)
      // The third wait is 4 s (t = 3.2 s now).
      await vi.advanceTimersByTimeAsync(3700)
      expect(resumeStream).toHaveBeenCalledTimes(2)
      await vi.advanceTimersByTimeAsync(300)
      await sending
      flush()

      expect(resumeStream).toHaveBeenCalledTimes(3)
      expect(pipeline.exchange()).toMatchObject({
        status: 'error',
        errorCode: 'connection_error',
        errorMessage: 'described:connection_error',
        retryable: true,
      })
      expect(spaceConnectivity()).toBe('unreachable')
      expect(pipeline.submitting()).toBe(false)
    })

    it('does not resume a drop before the server announced the turn: it is a retryable error at once', async () => {
      const { pipeline } = pipelineWith({ sendIdempotency: true })
      streamChat.mockImplementationOnce(async (_request: unknown, handlers: ChatStreamHandlers) => {
        handlers.onError?.({ code: 'connection_error', message: 'x' } as never)
      })
      await pipeline.send(input())
      flush()
      expect(resumeStream).not.toHaveBeenCalled()
      expect(pipeline.exchange()).toMatchObject({ status: 'error', retryable: true })
    })

    it('with the send key off a dropped stream is an error at once, never resumed, and not retryable', async () => {
      const { pipeline } = pipelineWith({ sendIdempotency: false })
      streamChat.mockImplementationOnce(async (_request: unknown, handlers: ChatStreamHandlers) => {
        handlers.onConnected?.({ requestId: 'req_5' } as never)
        handlers.onError?.({ code: 'connection_error', message: 'x' } as never)
      })
      await pipeline.send(input())
      flush()
      expect(resumeStream).not.toHaveBeenCalled()
      expect(pipeline.exchange()).toMatchObject({ status: 'error', errorCode: 'connection_error' })
      expect(pipeline.exchange()?.retryable).toBeUndefined()
    })

    it('a stop during the resume wait ends the wait and never resumes', async () => {
      vi.useFakeTimers()
      const { pipeline } = pipelineWith({ sendIdempotency: true })
      streamChat.mockImplementationOnce(async (_request: unknown, handlers: ChatStreamHandlers) => {
        handlers.onConnected?.({ requestId: 'req_5' } as never)
        handlers.onError?.({ code: 'connection_error', message: 'x' } as never)
      })
      cancelInvocation.mockResolvedValue(undefined)
      const sending = pipeline.send(input())
      await vi.advanceTimersByTimeAsync(100)
      await pipeline.stop()
      await vi.advanceTimersByTimeAsync(5000)
      await sending
      flush()
      expect(resumeStream).not.toHaveBeenCalled()
      expect(cancelInvocation).toHaveBeenCalledWith('req_5')
      expect(pipeline.exchange()).toMatchObject({ status: 'stopped' })
    })

    it('a retry after exhausted resumes reuses the first send id', async () => {
      vi.useFakeTimers()
      const { pipeline } = pipelineWith({ sendIdempotency: true })
      streamChat
        .mockImplementationOnce(async (_request: unknown, handlers: ChatStreamHandlers) => {
          handlers.onConnected?.({ requestId: 'req_5' } as never)
          handlers.onError?.({ code: 'connection_error', message: 'x' } as never)
        })
        .mockResolvedValueOnce(undefined)
      resumeStream.mockImplementation(async (_requestId: unknown, handlers: ChatStreamHandlers) => {
        handlers.onError?.({ code: 'connection_error', message: 'x' } as never)
      })
      const sending = pipeline.send(input())
      await vi.advanceTimersByTimeAsync(8000)
      await sending
      flush()
      const firstId = (streamChat.mock.calls[0]?.[0] as Record<string, unknown>).clientMessageId
      await pipeline.retry()
      expect(streamChat).toHaveBeenCalledTimes(2)
      expect((streamChat.mock.calls[1]?.[0] as Record<string, unknown>).clientMessageId).toBe(firstId)
    })
  })
})
