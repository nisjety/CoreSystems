import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type { ChatStreamHandlers } from '@/shared/api/chat-client'

// WS07-S11: the pipeline hands the sender's own live artifact to the host, through
// the room's kind and size gate, with the thread id the stream announced. The
// transport is mocked: no model, no network.

const streamChat = vi.fn()

vi.mock('@/shared/api/chat-client', () => ({
  streamChat: (...args: unknown[]) => streamChat(...args),
  resumeStream: vi.fn(),
  cancelInvocation: vi.fn(),
}))

vi.mock('@/shared/api/spaces-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/spaces-client')>()),
  searchSpaceMessages: () => new Promise(() => {}),
  updateSpaceThreadPresentation: vi.fn(async () => undefined),
  createSpacePost: vi.fn(),
  replyInSpaceThread: vi.fn(),
}))

import { createSpaceSendPipeline, type SpaceSendInput } from './space-send-pipeline'
import { noteSendTransportOk } from './space-connectivity'

const model = { model: 'test-model', label: 'Test model' } as const
const input = (overrides: Partial<SpaceSendInput> = {}): SpaceSendInput => ({
  content: 'hello',
  mode: 'ask',
  model,
  skills: [],
  ...overrides,
})

function pipelineWith(onArtifact?: (artifact: unknown, threadId: string) => void) {
  const host = onArtifact
    ? { spaceRef: 'room', onArtifact: onArtifact as never }
    : { spaceRef: 'room' }
  return createSpaceSendPipeline({
    host,
    resolveTier: async () => ({ tier: undefined, source: 'intent-mode' }),
    describeError: (event) => `described:${event.code ?? 'unknown'}`,
    sendIdempotency: () => false,
  })
}

const art = (over: Record<string, unknown> = {}) => ({
  id: 'a1',
  kind: 'document',
  title: 'Plan',
  content: 'body',
  version: 1,
  ...over,
})

beforeEach(() => {
  streamChat.mockReset()
  noteSendTransportOk()
})

afterEach(() => {
  vi.useRealTimers()
})

describe('space send pipeline live artifacts (WS07-S11)', () => {
  it('calls onArtifact with the gated artifact and the live thread id', async () => {
    const onArtifact = vi.fn()
    const pipeline = pipelineWith(onArtifact)
    streamChat.mockImplementationOnce(async (_request: unknown, handlers: ChatStreamHandlers) => {
      handlers.onConnected?.({ requestId: 'req_1', threadId: 'thr_9' } as never)
      handlers.onArtifact?.(art({ kind: 'code', version: 2 }) as never)
      handlers.onDone?.({} as never)
    })
    await pipeline.send(input())
    expect(onArtifact).toHaveBeenCalledTimes(1)
    expect(onArtifact).toHaveBeenCalledWith(
      { id: 'a1', kind: 'code', title: 'Plan', content: 'body', version: 2 },
      'thr_9',
    )
  })

  it('ignores pdf and image events and malformed ones', async () => {
    const onArtifact = vi.fn()
    const pipeline = pipelineWith(onArtifact)
    streamChat.mockImplementationOnce(async (_request: unknown, handlers: ChatStreamHandlers) => {
      handlers.onConnected?.({ requestId: 'req_1', threadId: 'thr_9' } as never)
      handlers.onArtifact?.(art({ id: 'p', kind: 'pdf' }) as never)
      handlers.onArtifact?.(art({ id: 'i', kind: 'image' }) as never)
      handlers.onArtifact?.(art({ id: 'z', version: 0 }) as never)
      handlers.onArtifact?.({} as never)
      handlers.onDone?.({} as never)
    })
    await pipeline.send(input())
    expect(onArtifact).not.toHaveBeenCalled()
  })

  it('holds nothing until the stream has announced a thread id', async () => {
    const onArtifact = vi.fn()
    const pipeline = pipelineWith(onArtifact)
    streamChat.mockImplementationOnce(async (_request: unknown, handlers: ChatStreamHandlers) => {
      handlers.onConnected?.({ requestId: 'req_1' } as never)
      handlers.onArtifact?.(art() as never)
      handlers.onDone?.({} as never)
    })
    await pipeline.send(input())
    expect(onArtifact).not.toHaveBeenCalled()
  })

  it('is a no-op without a host.onArtifact and does not break the exchange', async () => {
    const pipeline = pipelineWith()
    streamChat.mockImplementationOnce(async (_request: unknown, handlers: ChatStreamHandlers) => {
      handlers.onConnected?.({ requestId: 'req_1', threadId: 'thr_9' } as never)
      handlers.onArtifact?.(art() as never)
      handlers.onMessage?.({ content: 'Hi' } as never)
      handlers.onDone?.({} as never)
    })
    await pipeline.send(input())
    expect(pipeline.exchange()).toMatchObject({ assistantContent: 'Hi', status: 'done' })
  })
})
