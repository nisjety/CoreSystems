// @vitest-environment jsdom

/**
 * Taking back a mid-run message (O-01, DEC-F01-03), on the controller.
 *
 * The decisions are the gateway's: the queue answers `retracted`, `already_delivered` or `not_found`.
 * What this pins is what the controller does with each, in particular that a message whose take-back is
 * in flight is never resent by the run end, and that an edit puts the text back in the composer instead
 * of queueing it again. The harness mirrors `use-chat-controller.send-receipts.test.ts`.
 */

import { createRoot, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ChatStreamHandlers, QueuedInputResult, QueuedInputRetractResult } from '@/shared/api/chat-client'
import { __resetRetentionForTests } from '@/features/chat/lib/chat-retention'
import { settleQueuedInputsAtRunEnd } from './chat-normalizers'
import type { QueuedInput } from './chat-types'

type CapturedStream = {
  request: { content: string; clientMessageId?: string }
  handlers: ChatStreamHandlers
  resolve: () => void
}

const { streams, mockStreamChat, mockQueueInvocationInput, mockRetractQueuedInput } = vi.hoisted(() => {
  const streams: CapturedStream[] = []
  const mockStreamChat = vi.fn((request: CapturedStream['request'], handlers: ChatStreamHandlers) => {
    return new Promise<void>((resolve) => {
      streams.push({ request, handlers, resolve })
    })
  })
  const mockQueueInvocationInput = vi.fn<
    (requestId: string, content: string, options?: { clientMessageId?: string }) => Promise<QueuedInputResult>
  >()
  const mockRetractQueuedInput = vi.fn<
    (requestId: string, clientMessageId: string) => Promise<QueuedInputRetractResult>
  >()
  return { streams, mockStreamChat, mockQueueInvocationInput, mockRetractQueuedInput }
})

vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return {
    ...actual,
    streamChat: mockStreamChat,
    queueInvocationInput: mockQueueInvocationInput,
    retractQueuedInput: mockRetractQueuedInput,
    listModels: vi.fn().mockResolvedValue([]),
    listChatThreads: vi.fn().mockResolvedValue([]),
    saveChatThreadSnapshot: vi.fn().mockResolvedValue(null),
    getChatThreadTranscript: vi.fn().mockResolvedValue(null),
    getThreadMessages: vi.fn().mockResolvedValue([]),
    cancelInvocation: vi.fn().mockResolvedValue(undefined),
  }
})

import { useChatController } from './use-chat-controller'

afterEach(() => {
  streams.length = 0
  mockStreamChat.mockClear()
  mockQueueInvocationInput.mockReset()
  mockRetractQueuedInput.mockReset()
  __resetRetentionForTests()
  try {
    window.localStorage.clear()
  } catch {
    // ignore
  }
})

/** Flush pending microtasks (SSE-handler continuations, chained awaits) and Solid's batched store writes. */
async function tick(times = 3) {
  for (let i = 0; i < times; i += 1) {
    await Promise.resolve()
    flush()
  }
}

function withController<T>(fn: (controller: ReturnType<typeof useChatController>) => Promise<T>): Promise<T> {
  return new Promise((resolvePromise, rejectPromise) => {
    createRoot((dispose) => {
      const controller = useChatController()
      fn(controller).then(
        (value) => {
          dispose()
          resolvePromise(value)
        },
        (error) => {
          dispose()
          rejectPromise(error)
        },
      )
    })
  })
}

const send = (controller: ReturnType<typeof useChatController>, text: string) =>
  controller.handleComposerSubmit({ actions: [], attachments: [], text, tools: [] })

async function finishStreamWithUndelivered(stream: CapturedStream, requestId: string, undelivered: string[] | undefined) {
  stream.handlers.onDone?.({
    requestId,
    modelUsed: 'verevon-balance',
    outputTokens: 4,
    stopReason: 'stop',
    ...(undelivered ? { undeliveredClientMessageIds: undelivered } : {}),
  })
  stream.resolve()
  await tick()
}

/** A first turn that has connected, so a message typed now is a mid-run message. */
async function runningTurn(controller: ReturnType<typeof useChatController>) {
  await tick()
  await send(controller, 'Første spørsmål')
  await tick()
  streams[0]!.handlers.onMessage?.({ content: 'Første svar', requestId: 'req-1' })
  flush()
}

/** A mid-run message the gateway accepted, and the id it was sent under. */
async function acceptedMessage(controller: ReturnType<typeof useChatController>, text = 'Og hvem eier planen?') {
  mockQueueInvocationInput.mockResolvedValueOnce({ outcome: 'queued', pending: 1, persisted: true })
  await send(controller, text)
  await tick()
  return mockQueueInvocationInput.mock.calls.at(-1)![2]!.clientMessageId!
}

const states = (controller: ReturnType<typeof useChatController>) =>
  controller.state.queuedInputs.map((item) => item.state)

describe('the run end leaves a message whose take-back is in flight alone', () => {
  const pending = (overrides: Partial<QueuedInput> = {}): QueuedInput => ({
    id: 'queued-1',
    content: 'bruk EUR',
    state: 'pending',
    accepted: true,
    clientMessageId: 'msg-1',
    ...overrides,
  })

  it('neither resends it nor marks it delivered, whatever the done frame names', () => {
    const retracting = pending({ retracting: true })
    for (const undelivered of [undefined, ['msg-1'], []]) {
      const { entries, resend } = settleQueuedInputsAtRunEnd([retracting], undelivered)
      expect(resend).toEqual([])
      expect(entries).toEqual([retracting])
    }
  })

  it('still settles every other accepted message as before', () => {
    const { entries, resend } = settleQueuedInputsAtRunEnd([pending({ clientMessageId: 'msg-2' })], ['msg-2'])
    expect(resend.map((item) => item.clientMessageId)).toEqual(['msg-2'])
    expect(entries[0]!.state).toBe('resent')
  })
})

describe('the controller takes a mid-run message back', () => {
  it('removes the message the gateway gave back, and the run end sends nothing again', async () => {
    mockRetractQueuedInput.mockResolvedValueOnce({ outcome: 'retracted', withdrawn: true })
    await withController(async (controller) => {
      await runningTurn(controller)
      const clientMessageId = await acceptedMessage(controller)
      const [queued] = controller.state.queuedInputs

      await expect(controller.retractQueuedMessage(queued!)).resolves.toBe(true)
      expect(mockRetractQueuedInput).toHaveBeenCalledWith('req-1', clientMessageId)
      expect(controller.state.queuedInputs).toEqual([])

      // The run ends naming nothing taken: with the entry gone there is nothing to send again.
      await finishStreamWithUndelivered(streams[0]!, 'req-1', [clientMessageId])
      expect(streams, 'a message that was taken back is never sent as a new turn').toHaveLength(1)
    })
  })

  it('leaves a visible note in front of the streaming answer when the stored copy was withdrawn, and only then', async () => {
    mockRetractQueuedInput
      .mockResolvedValueOnce({ outcome: 'retracted', withdrawn: true })
      .mockResolvedValueOnce({ outcome: 'retracted', withdrawn: false })
    await withController(async (controller) => {
      await runningTurn(controller)
      await acceptedMessage(controller, 'Første ombestemmelse')
      await controller.retractQueuedMessage(controller.state.queuedInputs[0]!)
      await tick()
      const turns = controller.state.turns
      const noteAt = turns.findIndex((turn) => turn.retracted)
      expect(noteAt, 'a text-free note, before the answer that is still streaming').toBeGreaterThan(0)
      expect(turns[noteAt]).toMatchObject({ role: 'user', content: '', retracted: true })
      expect(turns[noteAt + 1]?.role).toBe('assistant')
      expect(turns.filter((turn) => turn.retracted)).toHaveLength(1)

      await acceptedMessage(controller, 'Andre ombestemmelse')
      await controller.retractQueuedMessage(controller.state.queuedInputs[0]!)
      await tick()
      expect(
        controller.state.turns.filter((turn) => turn.retracted),
        'nothing was withdrawn in the thread, so no second note',
      ).toHaveLength(1)
    })
  })

  it('does not resend a message whose take-back is still in flight when the run ends', async () => {
    let answer!: (result: QueuedInputRetractResult) => void
    mockRetractQueuedInput.mockReturnValueOnce(new Promise((resolve) => { answer = resolve }))
    await withController(async (controller) => {
      await runningTurn(controller)
      const clientMessageId = await acceptedMessage(controller)
      const taking = controller.retractQueuedMessage(controller.state.queuedInputs[0]!)
      await tick()
      expect(controller.state.queuedInputs[0]!.retracting).toBe(true)

      await finishStreamWithUndelivered(streams[0]!, 'req-1', [clientMessageId])
      expect(streams, 'the run end leaves it to the take-back').toHaveLength(1)

      answer({ outcome: 'retracted', withdrawn: true })
      await expect(taking).resolves.toBe(true)
      await tick()
      expect(controller.state.queuedInputs).toEqual([])
      expect(streams).toHaveLength(1)
    })
  })

  it('keeps the message when the run took it first, and says it can no longer be taken back', async () => {
    mockRetractQueuedInput.mockResolvedValueOnce({ outcome: 'already_delivered' })
    await withController(async (controller) => {
      await runningTurn(controller)
      const clientMessageId = await acceptedMessage(controller)

      await expect(controller.retractQueuedMessage(controller.state.queuedInputs[0]!)).resolves.toBe(false)
      const [kept] = controller.state.queuedInputs
      expect(kept).toMatchObject({ state: 'pending', accepted: true, retracting: false })
      expect(kept!.note).toBeTruthy()

      // The delivery frame settles it, as it always did.
      streams[0]!.handlers.onQueuedInput?.({ messages: ['Og hvem eier planen?'], ids: [clientMessageId] })
      flush()
      expect(states(controller)).toEqual(['delivered'])
    })
  })

  it('settles the run end it missed: a message the done frame named is sent again, one it did not name is not', async () => {
    let answer!: (result: QueuedInputRetractResult) => void
    mockRetractQueuedInput.mockReturnValueOnce(new Promise((resolve) => { answer = resolve }))
    await withController(async (controller) => {
      await runningTurn(controller)
      const clientMessageId = await acceptedMessage(controller)
      const taking = controller.retractQueuedMessage(controller.state.queuedInputs[0]!)
      await tick()
      await finishStreamWithUndelivered(streams[0]!, 'req-1', [clientMessageId])

      // The gateway says it is no longer the run's to withdraw (the done frame handed it over to a resend).
      answer({ outcome: 'already_delivered' })
      await taking
      await tick(10)
      expect(controller.state.queuedInputs.map((item) => item.state)).toEqual(['resent'])
      expect(streams, 'the named message goes out as a new turn, under its own id').toHaveLength(2)
      expect(streams[1]!.request.clientMessageId).toBe(clientMessageId)
    })
  })

  it('keeps the message and says so when the gateway could not decide', async () => {
    mockRetractQueuedInput.mockResolvedValueOnce({ outcome: 'refused', reason: 'unavailable', message: 'busy' })
    await withController(async (controller) => {
      await runningTurn(controller)
      await acceptedMessage(controller)

      await expect(controller.retractQueuedMessage(controller.state.queuedInputs[0]!)).resolves.toBe(false)
      expect(controller.state.queuedInputs[0]).toMatchObject({ state: 'pending', accepted: true, retracting: false })
      expect(controller.state.queuedInputs[0]!.note).toBeTruthy()

      // Asking again is allowed: nothing was decided.
      mockRetractQueuedInput.mockResolvedValueOnce({ outcome: 'retracted', withdrawn: true })
      await expect(controller.retractQueuedMessage(controller.state.queuedInputs[0]!)).resolves.toBe(true)
    })
  })

  it('asks only about a message the gateway accepted and has not delivered, under its id', async () => {
    await withController(async (controller) => {
      await runningTurn(controller)
      mockQueueInvocationInput.mockResolvedValueOnce({ outcome: 'refused', reason: 'too_many', message: 'for mange' })
      await send(controller, 'for mange')
      await tick()
      await expect(controller.retractQueuedMessage(controller.state.queuedInputs[0]!)).resolves.toBe(false)
      expect(mockRetractQueuedInput).not.toHaveBeenCalled()
    })
  })

  it('an edit puts the edited text back in the composer once the message is taken back, and never queues it again', async () => {
    mockRetractQueuedInput.mockResolvedValueOnce({ outcome: 'retracted', withdrawn: true })
    await withController(async (controller) => {
      await runningTurn(controller)
      await acceptedMessage(controller)
      controller.setInput('')

      await controller.editQueuedMessage(controller.state.queuedInputs[0]!, 'Og hvem eier den nye planen?')
      expect(controller.input()).toBe('Og hvem eier den nye planen?')
      expect(controller.state.queuedInputs).toEqual([])
      expect(mockQueueInvocationInput, 'the edit never queues anything by itself').toHaveBeenCalledTimes(1)
    })
  })

  it('an edit that did not take the message back leaves the composer alone', async () => {
    mockRetractQueuedInput.mockResolvedValueOnce({ outcome: 'already_delivered' })
    await withController(async (controller) => {
      await runningTurn(controller)
      await acceptedMessage(controller)
      controller.setInput('noe jeg skriver nå')

      await controller.editQueuedMessage(controller.state.queuedInputs[0]!, 'Og hvem eier den nye planen?')
      expect(controller.input()).toBe('noe jeg skriver nå')
    })
  })
})

describe('a take-back whose stored copy the store could not mark withdrawn is asked again (O-01, rel1)', () => {
  // The queue has given the message back, so the run never reads it; only the stored copy is still live. The controller
  // asks the same take-back again in the background (on fake timers here), and the entry stays in the strip until then.
  const retryable = { outcome: 'retracted', withdrawn: false, retryable: true } as const

  beforeEach(() => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] })
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('shows the note once the same take-back marks the stored copy withdrawn, and the run end sends nothing again', async () => {
    mockRetractQueuedInput
      .mockResolvedValueOnce(retryable)
      .mockResolvedValueOnce({ outcome: 'retracted', withdrawn: true })
    await withController(async (controller) => {
      await runningTurn(controller)
      const clientMessageId = await acceptedMessage(controller)

      await expect(controller.retractQueuedMessage(controller.state.queuedInputs[0]!)).resolves.toBe(true)
      expect(controller.state.queuedInputs).toMatchObject([{ state: 'pending', retracting: true }])
      expect(controller.state.turns.some((turn) => turn.retracted), 'nothing is withdrawn yet').toBe(false)

      await vi.advanceTimersByTimeAsync(1_000)
      await tick()
      expect(mockRetractQueuedInput).toHaveBeenCalledTimes(2)
      expect(mockRetractQueuedInput).toHaveBeenLastCalledWith('req-1', clientMessageId)
      expect(controller.state.queuedInputs).toEqual([])
      const noteAt = controller.state.turns.findIndex((turn) => turn.retracted)
      expect(noteAt, 'a text-free note, before the answer that is still streaming').toBeGreaterThan(0)
      expect(controller.state.turns[noteAt + 1]?.role).toBe('assistant')

      await finishStreamWithUndelivered(streams[0]!, 'req-1', [clientMessageId])
      expect(streams, 'a message that was taken back is never sent as a new turn').toHaveLength(1)
    })
  })

  it('leaves the message alone when the run ends meanwhile, and says so when the ask finds no run', async () => {
    mockRetractQueuedInput
      .mockResolvedValueOnce(retryable)
      .mockResolvedValueOnce({ outcome: 'not_found' })
    await withController(async (controller) => {
      await runningTurn(controller)
      const clientMessageId = await acceptedMessage(controller)
      await controller.retractQueuedMessage(controller.state.queuedInputs[0]!)

      await finishStreamWithUndelivered(streams[0]!, 'req-1', [clientMessageId])
      expect(streams, 'the run end leaves it to the take-back').toHaveLength(1)
      expect(controller.state.queuedInputs).toMatchObject([{ retracting: true }])

      await vi.advanceTimersByTimeAsync(1_000)
      await tick()
      expect(mockRetractQueuedInput).toHaveBeenCalledTimes(2)
      expect(streams, 'a message that was taken back is never sent as a new turn').toHaveLength(1)
      expect(controller.state.queuedInputs).toMatchObject([{ state: 'refused', retracting: false }])
      expect(controller.state.queuedInputs[0]!.note).toBeTruthy()
      expect(controller.state.turns.some((turn) => turn.retracted), 'nothing was withdrawn, so no note').toBe(false)
    })
  })

  it('asks a bounded number of times, then gives up with a note the person can see', async () => {
    mockRetractQueuedInput.mockResolvedValue(retryable)
    await withController(async (controller) => {
      await runningTurn(controller)
      await acceptedMessage(controller)
      await controller.retractQueuedMessage(controller.state.queuedInputs[0]!)

      for (const wait of [1_000, 2_000, 4_000, 8_000]) {
        await vi.advanceTimersByTimeAsync(wait)
        await tick()
      }
      expect(mockRetractQueuedInput, 'the first ask and four retries').toHaveBeenCalledTimes(5)
      expect(controller.state.queuedInputs).toMatchObject([{ state: 'refused', retracting: false }])
      expect(controller.state.queuedInputs[0]!.note).toBeTruthy()

      await vi.advanceTimersByTimeAsync(60_000)
      await tick()
      expect(mockRetractQueuedInput, 'no ask after the last one').toHaveBeenCalledTimes(5)
    })
  })

  it('does not ask again when the stored copy stays because an answer already follows it', async () => {
    mockRetractQueuedInput.mockResolvedValueOnce({ outcome: 'retracted', withdrawn: false })
    await withController(async (controller) => {
      await runningTurn(controller)
      await acceptedMessage(controller)

      await expect(controller.retractQueuedMessage(controller.state.queuedInputs[0]!)).resolves.toBe(true)
      expect(controller.state.queuedInputs).toEqual([])
      await vi.advanceTimersByTimeAsync(60_000)
      await tick()
      expect(mockRetractQueuedInput).toHaveBeenCalledTimes(1)
      expect(controller.state.turns.some((turn) => turn.retracted)).toBe(false)
    })
  })

  it('an edit puts the text back at once, because the queue has given the message back', async () => {
    mockRetractQueuedInput.mockResolvedValueOnce(retryable)
    await withController(async (controller) => {
      await runningTurn(controller)
      await acceptedMessage(controller)
      controller.setInput('')

      await controller.editQueuedMessage(controller.state.queuedInputs[0]!, 'Og hvem eier den nye planen?')
      expect(controller.input()).toBe('Og hvem eier den nye planen?')
      expect(mockQueueInvocationInput, 'the edit never queues anything by itself').toHaveBeenCalledTimes(1)
    })
  })
})
