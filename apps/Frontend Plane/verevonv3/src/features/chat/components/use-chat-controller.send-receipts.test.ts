// @vitest-environment jsdom

/**
 * Durable send receipts (WS04 S6): every send is named, the gateway stores the message under
 * its id, and the mid-run tray reports delivered, not delivered or resent.
 *
 * The decisions live in pure helpers (`chat-normalizers.ts`), pinned first. The hook is then
 * driven through its public surface with a stubbed `streamChat` and `queueInvocationInput`
 * (the stubs and helpers mirror `use-chat-controller.follow-up.test.ts`), and the wiring that
 * has no seam is pinned as source text, the way `mid-run-input.test.ts` pins the rest of the
 * mid-run arc.
 */

import { createRoot, flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { ChatStreamHandlers, QueuedInputResult } from '@/shared/api/chat-client'
import { __resetRetentionForTests } from '@/features/chat/lib/chat-retention'
import {
  markQueuedInputsDelivered,
  queuedInputsForNewTurn,
  sendClientMessageId,
  settleQueuedInputsAtRunEnd,
} from './chat-normalizers'
import type { QueuedInput } from './chat-types'

type CapturedStream = {
  request: { content: string; clientMessageId?: string }
  handlers: ChatStreamHandlers
  resolve: () => void
}

const { streams, mockStreamChat, mockQueueInvocationInput } = vi.hoisted(() => {
  const streams: CapturedStream[] = []
  const mockStreamChat = vi.fn((request: CapturedStream['request'], handlers: ChatStreamHandlers) => {
    return new Promise<void>((resolve) => {
      streams.push({ request, handlers, resolve })
    })
  })
  const mockQueueInvocationInput = vi.fn<
    (requestId: string, content: string, options?: { clientMessageId?: string }) => Promise<QueuedInputResult>
  >()
  return { streams, mockStreamChat, mockQueueInvocationInput }
})

vi.mock('@/shared/api/chat-client', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/chat-client')>()
  return {
    ...actual,
    streamChat: mockStreamChat,
    queueInvocationInput: mockQueueInvocationInput,
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
  __resetRetentionForTests()
  try {
    window.localStorage.clear()
  } catch {
    // ignore
  }
})

const entry = (overrides: Partial<QueuedInput> = {}): QueuedInput => ({
  id: 'queued-1',
  content: 'bruk EUR i stedet',
  state: 'pending',
  ...overrides,
})

describe('the id a send is named with', () => {
  const mint = () => 'msg-minted'

  it('names an ordinary send', () => {
    expect(sendClientMessageId({}, mint)).toBe('msg-minted')
  })

  it('keeps the id of a send that repeats an earlier one, so both land as one transcript row', () => {
    expect(sendClientMessageId({ clientMessageId: 'msg-first' }, mint)).toBe('msg-first')
  })

  it('names nothing for a zero-retention turn: an id is a stored fact and nothing of the turn is stored', () => {
    expect(sendClientMessageId({ zdr: true }, mint)).toBeUndefined()
    expect(sendClientMessageId({ zdr: true, clientMessageId: 'msg-first' }, mint)).toBeUndefined()
  })

  it('names nothing for a regenerate or an edited resubmit, which restate the previous question on purpose', () => {
    expect(sendClientMessageId({ regenerated: true }, mint)).toBeUndefined()
    expect(sendClientMessageId({ editResubmit: true }, mint)).toBeUndefined()
  })

  it('names nothing for a send that appends no user turn', () => {
    expect(sendClientMessageId({ appendUser: false }, mint)).toBeUndefined()
  })

  it('mints ids the gateway will store', () => {
    // The shape session-core allows: [A-Za-z0-9][A-Za-z0-9_-]{0,127}.
    const stored = /^[A-Za-z0-9][A-Za-z0-9_-]{0,127}$/
    return import('./chat-normalizers').then(({ createId }) => {
      expect(stored.test(createId('msg'))).toBe(true)
    })
  })
})

describe('a delivery names the messages it carries', () => {
  it('promotes by id, and stays right when two messages say the same words', () => {
    const entries = [
      entry({ id: 'q1', clientMessageId: 'msg-1' }),
      entry({ id: 'q2', clientMessageId: 'msg-2' }),
    ]
    const after = markQueuedInputsDelivered(entries, ['bruk EUR i stedet'], ['msg-2'])
    expect(after.map((item) => item.state)).toEqual(['pending', 'delivered'])
  })

  it('promotes by text when the delivery carries no ids', () => {
    const entries = [entry({ id: 'q1', clientMessageId: 'msg-1' }), entry({ id: 'q2', content: 'noe annet' })]
    const after = markQueuedInputsDelivered(entries, ['  bruk EUR i stedet '])
    expect(after.map((item) => item.state)).toEqual(['delivered', 'pending'])
  })

  it('promotes only a pending message', () => {
    const entries = [entry({ state: 'refused', note: 'for lang' }), entry({ id: 'q2', state: 'resent' })]
    expect(markQueuedInputsDelivered(entries, ['bruk EUR i stedet'])).toEqual(entries)
  })

  it('clears the note of a delivered message', () => {
    const [after] = markQueuedInputsDelivered(
      [entry({ clientMessageId: 'msg-1', note: 'Levert, men ikke lagret i samtalehistorikken.' })],
      ['bruk EUR i stedet'],
      ['msg-1'],
    )
    expect(after).toMatchObject({ state: 'delivered', note: undefined })
  })
})

describe('the run ends', () => {
  it('sends again, under the same id, a message the gateway accepted and the run never took', () => {
    const taken = entry({ id: 'q1', clientMessageId: 'msg-1', accepted: true, state: 'delivered' })
    const leftover = entry({ id: 'q2', clientMessageId: 'msg-2', accepted: true })
    const { entries, resend } = settleQueuedInputsAtRunEnd([taken, leftover])

    expect(resend).toEqual([leftover])
    expect(entries.map((item) => item.state)).toEqual(['delivered', 'resent'])
    // Settled once: the same entries at the next run's end send nothing again.
    expect(settleQueuedInputsAtRunEnd(entries).resend).toEqual([])
  })

  it('leaves alone a message that was refused, or never reached the gateway', () => {
    const refused = entry({ id: 'q1', state: 'refused', note: 'for mange i kø', accepted: false })
    // Waiting for the run to start: already in the queue of sends, so it must not go out twice.
    const waiting = entry({ id: 'q2', note: 'Venter på at kjøringen starter.' })
    const alreadyResent = entry({ id: 'q3', state: 'resent' })
    const { entries, resend } = settleQueuedInputsAtRunEnd([refused, waiting, alreadyResent])
    expect(resend).toEqual([])
    expect(entries).toEqual([refused, waiting, alreadyResent])
  })

  it('keeps the entries that say "resent" through the turn that resends them, and clears the rest', () => {
    const entries = [
      entry({ id: 'q1', state: 'delivered' }),
      entry({ id: 'q2', state: 'resent' }),
      entry({ id: 'q3', state: 'refused' }),
    ]
    expect(queuedInputsForNewTurn(entries, true).map((item) => item.id)).toEqual(['q2'])
    expect(queuedInputsForNewTurn(entries, false)).toEqual([])
  })
})

/** The wiring has no seam inside the hook; each property is pinned where it lives. */
describe('the controller wires receipts through the send path', () => {
  async function controller(): Promise<string> {
    const { readFileSync } = await import('node:fs')
    const { resolve } = await import('node:path')
    return readFileSync(resolve(process.cwd(), 'src/features/chat/components/use-chat-controller.ts'), 'utf8')
      .split(String.fromCharCode(13))
      .join('')
  }

  it('names every ordinary send, and sends the id on the stream request', async () => {
    const text = await controller()
    expect(text).toContain('clientMessageId: sendClientMessageId(options, () => createId(\'msg\')),')
  })

  it('names a mid-run message, and never for a zero-retention thread', async () => {
    const text = await controller()
    const delivery = text.slice(text.indexOf('const deliverMidRun'), text.indexOf('const sendContent = async'))
    expect(delivery).toContain('sendClientMessageId(')
    expect(delivery).toContain('isTemporaryThread(state.threadId)')
    expect(delivery).toContain('clientMessageId,\n      })')
  })

  it('sends a message the run ended before receiving under the same id and marks it resent', async () => {
    const text = await controller()
    const delivery = text.slice(text.indexOf('const deliverMidRun'), text.indexOf('const sendContent = async'))
    expect(delivery).toContain('options: { ...options, clientMessageId, resendOfQueued: true }')
    expect(delivery).toContain("noteQueuedInput(id, 'resent'")
  })

  it('sends again a message the run never took, when the run is done, and only then', async () => {
    const text = await controller()
    const doneStart = text.indexOf("markOpenSteps('done', 'Completed.', assistantId)\n            // The run is over")
    expect(doneStart, 'the live stream no longer settles its mid-run messages at done').toBeGreaterThan(-1)
    expect(text.slice(doneStart, doneStart + 400)).toContain('resendUntakenQueuedInputs()')
    // One call site: a stop, an error and a dropped connection never resend a message the user
    // may have meant to stop with the run.
    expect(text.match(/resendUntakenQueuedInputs\(\)/g)).toHaveLength(1)
  })

  it('keeps the "resent" entries through the resending turn', async () => {
    const text = await controller()
    expect(text).toContain('queuedInputsForNewTurn(s.queuedInputs, options.resendOfQueued === true)')
  })

  it('matches a delivery by id before text', async () => {
    const text = await controller()
    const handler = text.slice(text.indexOf('onQueuedInput:'), text.indexOf('onMemoryRecall:'))
    expect(handler).toContain('markQueuedInputsDelivered(s.queuedInputs, messages, ids)')
  })
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

async function finishStream(stream: CapturedStream, requestId: string) {
  stream.handlers.onDone?.({ requestId, modelUsed: 'verevon-balance', outputTokens: 4, stopReason: 'stop' })
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

const states = (controller: ReturnType<typeof useChatController>) =>
  controller.state.queuedInputs.map((item) => item.state)

describe('the controller names every send and settles the mid-run tray', () => {
  it('names an ordinary send, and a second send gets its own id', async () => {
    await withController(async (controller) => {
      await tick()
      await send(controller, 'Første spørsmål')
      await tick()
      await finishStream(streams[0]!, 'req-1')
      await send(controller, 'Andre spørsmål')
      await tick()

      const [first, second] = [streams[0]!.request.clientMessageId, streams[1]!.request.clientMessageId]
      expect(first).toMatch(/^msg-/)
      expect(second).toMatch(/^msg-/)
      expect(second).not.toBe(first)
    })
  })

  it('names a mid-run message, and the delivery that names it marks it delivered, with no resend', async () => {
    mockQueueInvocationInput.mockResolvedValueOnce({ outcome: 'queued', pending: 1, persisted: true })
    await withController(async (controller) => {
      await runningTurn(controller)
      await send(controller, 'Og hvem eier planen?')
      await tick()

      const queuedId = mockQueueInvocationInput.mock.calls[0]![2]!.clientMessageId
      expect(queuedId).toMatch(/^msg-/)
      expect(states(controller)).toEqual(['pending'])

      streams[0]!.handlers.onQueuedInput?.({ messages: ['Og hvem eier planen?'], ids: [queuedId!] })
      flush()
      expect(states(controller)).toEqual(['delivered'])

      await finishStream(streams[0]!, 'req-1')
      expect(streams, 'a message the run took is never sent again').toHaveLength(1)
      expect(states(controller)).toEqual(['delivered'])
    })
  })

  /**
   * The end-of-run leftover: accepted (and stored by the gateway), never taken by the run. It
   * goes out as an ordinary turn under the same id, so the transcript holds it once and the
   * agent answers it, and the tray says so instead of leaving it waiting.
   */
  it('sends again, under the same id, a message the run ended without taking', async () => {
    mockQueueInvocationInput.mockResolvedValueOnce({ outcome: 'queued', pending: 1, persisted: true })
    await withController(async (controller) => {
      await runningTurn(controller)
      await send(controller, 'Og hvem eier planen?')
      await tick()
      const queuedId = mockQueueInvocationInput.mock.calls[0]![2]!.clientMessageId

      await finishStream(streams[0]!, 'req-1')

      expect(streams).toHaveLength(2)
      expect(streams[1]!.request.content).toContain('Og hvem eier planen?')
      expect(streams[1]!.request.clientMessageId).toBe(queuedId)
      expect(states(controller), 'the strip survives the turn that resends it').toEqual(['resent'])

      // That turn ends too: nothing is sent a third time.
      await finishStream(streams[1]!, 'req-2')
      expect(streams).toHaveLength(2)
    })
  })

  it('sends again a message the run ended before receiving, under the id it was queued with', async () => {
    mockQueueInvocationInput.mockResolvedValueOnce({ outcome: 'run_ended', persisted: true })
    await withController(async (controller) => {
      await runningTurn(controller)
      await send(controller, 'Og hvem eier planen?')
      await tick()
      const queuedId = mockQueueInvocationInput.mock.calls[0]![2]!.clientMessageId
      await finishStream(streams[0]!, 'req-1')

      expect(streams).toHaveLength(2)
      expect(streams[1]!.request.clientMessageId).toBe(queuedId)
      expect(states(controller)).toEqual(['resent'])
    })
  })

  it('does not send again a message the gateway refused', async () => {
    mockQueueInvocationInput.mockResolvedValueOnce({
      outcome: 'refused',
      reason: 'too_many',
      message: 'for mange i kø',
    })
    await withController(async (controller) => {
      await runningTurn(controller)
      await send(controller, 'Og hvem eier planen?')
      await tick()
      expect(states(controller)).toEqual(['refused'])

      await finishStream(streams[0]!, 'req-1')
      expect(streams).toHaveLength(1)
      expect(states(controller)).toEqual(['refused'])
    })
  })
})
