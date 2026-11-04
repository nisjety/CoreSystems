import { createRoot, flush } from 'solid-js'
import { describe, expect, it, vi } from 'vitest'

import type { QueuedInputResult } from '@/shared/api/chat-client'
import type { Skill } from '@/shared/api/skills-client'

import {
  createSpaceMidRun,
  deferredSendReady,
  type SpaceDeferredSend,
  type SpaceQueueCall,
  type SpaceSteerTarget,
} from './space-mid-run'

const TARGET: SpaceSteerTarget = { requestId: 'req-1', threadId: 'thr-1', spaceRef: 'room-1' }

// Solid 2 batches signal writes until a flush, and a read outside a reactive scope sees the last
// committed value. Every step below is followed by `flush()` before the strip is read.
function setup(options: { ended?: boolean } = {}) {
  const queue = vi.fn<SpaceQueueCall>()
  const state = { ended: options.ended ?? false, minted: 0 }
  const midRun = createRoot(() =>
    createSpaceMidRun({
      tr: (_no, en) => en,
      queue,
      mint: () => {
        state.minted += 1
        return `cm-${state.minted}`
      },
      settled: () => state.ended,
    }),
  )
  return {
    queue,
    midRun,
    end: () => {
      state.ended = true
    },
  }
}

describe('createSpaceMidRun: a steered message (WS09-S2)', () => {
  it('is named, aimed at the room thread, and shown pending once the gateway accepts it', async () => {
    const { queue, midRun } = setup()
    queue.mockResolvedValueOnce({ outcome: 'queued', pending: 1, persisted: true })

    const outcome = await midRun.steer('Bruk gul ledning', TARGET)
    flush()

    expect(outcome).toBe('queued')
    expect(queue).toHaveBeenCalledWith('req-1', 'Bruk gul ledning', {
      threadId: 'thr-1',
      spaceRef: 'room-1',
      clientMessageId: 'cm-1',
    })
    expect(midRun.entries()).toEqual([
      expect.objectContaining({ content: 'Bruk gul ledning', state: 'pending', accepted: true, clientMessageId: 'cm-1' }),
    ])
  })

  it('is delivered when the agent takes it, matched by its id', async () => {
    const { queue, midRun } = setup()
    queue.mockResolvedValueOnce({ outcome: 'queued', pending: 1, persisted: true })
    await midRun.steer('Bruk gul ledning', TARGET)

    midRun.markDelivered(['Bruk gul ledning'], ['cm-1'])
    flush()

    expect(midRun.entries()[0]?.state).toBe('delivered')
    expect(midRun.settle()).toEqual([])
  })

  it('is delivered by its text when the delivery names no ids', async () => {
    const { queue, midRun } = setup()
    queue.mockResolvedValueOnce({ outcome: 'queued', pending: 1, persisted: true })
    await midRun.steer('Bruk gul ledning', TARGET)

    midRun.markDelivered(['  Bruk gul ledning  '])
    flush()

    expect(midRun.entries()[0]?.state).toBe('delivered')
  })

  it('stays delivered, and is never sent again, when the agent takes it before the gateway answer arrives', async () => {
    const { queue, midRun } = setup()
    let answer: (result: QueuedInputResult) => void = () => {}
    queue.mockReturnValueOnce(new Promise<QueuedInputResult>((resolve) => {
      answer = resolve
    }))

    const steering = midRun.steer('Bruk gul ledning', TARGET)
    flush()
    midRun.markDelivered(['Bruk gul ledning'], ['cm-1'])
    flush()
    answer({ outcome: 'queued', pending: 1, persisted: true })
    const outcome = await steering
    flush()

    expect(outcome).toBe('queued')
    expect(midRun.entries()[0]?.state).toBe('delivered')
    expect(midRun.settle()).toEqual([])
  })

  it('is held for a resend when the turn had already ended before the answer arrived', async () => {
    const { queue, midRun, end } = setup()
    queue.mockResolvedValueOnce({ outcome: 'queued', pending: 1, persisted: true })
    end()

    const outcome = await midRun.steer('Bruk gul ledning', TARGET)
    flush()

    expect(outcome).toBe('resend')
    expect(midRun.entries()[0]?.state).toBe('resent')
    expect(midRun.settle()).toEqual([{ content: 'Bruk gul ledning', clientMessageId: 'cm-1' }])
    expect(midRun.settle()).toEqual([])
  })

  it('is held for a resend under the id the gateway stored it with, when the run ended before taking it', async () => {
    const { queue, midRun } = setup()
    queue.mockResolvedValueOnce({ outcome: 'run_ended', clientMessageId: 'cm-stored' })

    const outcome = await midRun.steer('Legg til kolonne', TARGET)
    flush()

    expect(outcome).toBe('resend')
    expect(midRun.entries()[0]).toEqual(expect.objectContaining({ state: 'resent' }))
    expect(midRun.settle()).toEqual([{ content: 'Legg til kolonne', clientMessageId: 'cm-stored' }])
  })

  it('is refused with the gateway reason, and is never resent', async () => {
    const { queue, midRun } = setup()
    queue.mockResolvedValueOnce({ outcome: 'refused', reason: 'too_many', message: 'Tre meldinger venter allerede.' })

    const outcome = await midRun.steer('Fjerde', TARGET)
    flush()

    expect(outcome).toBe('refused')
    expect(midRun.entries()[0]).toEqual(expect.objectContaining({ state: 'refused', note: 'Tre meldinger venter allerede.' }))
    expect(midRun.settle()).toEqual([])
  })

  it('is refused at once, with no gateway call, while the run has not started', async () => {
    const { queue, midRun } = setup()

    const outcome = await midRun.steer('Tidlig', { ...TARGET, requestId: undefined })
    flush()

    expect(outcome).toBe('refused')
    expect(queue).not.toHaveBeenCalled()
    expect(midRun.entries()[0]).toEqual(expect.objectContaining({ state: 'refused' }))
    expect(midRun.entries()[0]?.note).toContain('has not started yet')
  })

  it('is refused, not thrown, when the request fails before the gateway answers', async () => {
    const { queue, midRun } = setup()
    queue.mockRejectedValueOnce(new Error('offline'))

    const outcome = await midRun.steer('Hei', TARGET)
    flush()

    expect(outcome).toBe('refused')
    expect(midRun.entries()[0]).toEqual(
      expect.objectContaining({ state: 'refused', note: 'The message could not be sent.' }),
    )
  })
})

describe('createSpaceMidRun: settling the turn (WS09-S2)', () => {
  it('on a clean finish sends each message the run never read once, in order, and leaves delivered ones alone', async () => {
    const { queue, midRun } = setup()
    queue
      .mockResolvedValueOnce({ outcome: 'queued', pending: 1, persisted: true })
      .mockResolvedValueOnce({ outcome: 'queued', pending: 2, persisted: true })
    await midRun.steer('Første', TARGET)
    await midRun.steer('Andre', TARGET)
    midRun.markDelivered(['Andre'], ['cm-2'])
    flush()

    expect(midRun.settle()).toEqual([{ content: 'Første', clientMessageId: 'cm-1' }])
    flush()
    expect(midRun.entries().map((entry) => entry.state)).toEqual(['resent', 'delivered'])
    expect(midRun.settle()).toEqual([])
  })

  it('on a stop or a failure sends nothing again, and marks what the run never read as not sent again', async () => {
    const { queue, midRun } = setup()
    queue
      .mockResolvedValueOnce({ outcome: 'queued', pending: 1, persisted: true })
      .mockResolvedValueOnce({ outcome: 'run_ended' })
    await midRun.steer('Tatt', TARGET)
    await midRun.steer('Ferdig', TARGET)
    flush()

    midRun.abandon([])
    flush()

    expect(midRun.entries().map((entry) => entry.state)).toEqual(['refused', 'refused'])
    expect(midRun.entries()[0]?.note).toContain('was not sent again')
    expect(midRun.settle()).toEqual([])
  })

  it('refuses a resend still waiting its turn when that turn ends without a clean finish', async () => {
    const { queue, midRun } = setup()
    queue
      .mockResolvedValueOnce({ outcome: 'run_ended' })
      .mockResolvedValueOnce({ outcome: 'run_ended' })
    await midRun.steer('Ett', TARGET)
    await midRun.steer('To', TARGET)
    flush()

    const [first, ...waiting] = midRun.settle()
    midRun.abandon(waiting)
    flush()

    expect(first).toEqual({ content: 'Ett', clientMessageId: 'cm-1' })
    expect(midRun.entries().map((entry) => entry.state)).toEqual(['resent', 'refused'])
  })

  it('starts a new turn with an empty strip, and a resend turn keeps only the entries that say they are being resent', async () => {
    const { queue, midRun } = setup()
    queue
      .mockResolvedValueOnce({ outcome: 'queued', pending: 1, persisted: true })
      .mockResolvedValueOnce({ outcome: 'run_ended' })
    await midRun.steer('Tatt', TARGET)
    await midRun.steer('Ferdig', TARGET)
    flush()

    midRun.startTurn(true)
    flush()
    expect(midRun.entries().map((entry) => entry.content)).toEqual(['Ferdig'])

    midRun.startTurn(false)
    flush()
    expect(midRun.entries()).toEqual([])
  })
})

describe('createSpaceMidRun: the send-when-ready message (WS09-S2)', () => {
  it('is held once, and handed over once', () => {
    const { midRun } = setup()
    const waiting: SpaceDeferredSend = { content: 'Oppdater lista', threadId: 'thr-1' }

    midRun.defer(waiting)
    flush()
    expect(midRun.deferred()).toEqual(waiting)

    expect(midRun.takeDeferred()).toEqual(waiting)
    flush()
    expect(midRun.deferred()).toBeUndefined()
    expect(midRun.takeDeferred()).toBeUndefined()
  })

  it('keeps the skills and mention picks it was put to wait with, for a cancel to give back', () => {
    const { midRun } = setup()
    const waiting: SpaceDeferredSend = {
      content: '@Kari @Drift se på lista',
      threadId: 'thr-1',
      runId: 'run-9',
      mentionedAgentRef: 'agent-1',
      skills: [{ id: 'sk-1', name: 'Innkjøpsrutine' } as Skill],
      picks: {
        people: [{ subjectId: 'user-2', label: 'Kari' }],
        agents: [{ subjectId: 'agent-1', label: 'Drift', source: 'typed' }],
      },
    }

    midRun.defer(waiting)
    flush()

    expect(midRun.takeDeferred()).toEqual(waiting)
  })

  it('is dropped by a cancel, and is never sent', () => {
    const { queue, midRun } = setup()
    midRun.defer({ content: 'Vent', threadId: 'thr-1' })
    midRun.cancelDeferred()
    flush()

    expect(midRun.deferred()).toBeUndefined()
    expect(queue).not.toHaveBeenCalled()
  })
})

describe('deferredSendReady: when a send-when-ready goes out (WS09-S2)', () => {
  const waiting: SpaceDeferredSend = { content: 'Oppdater lista', threadId: 'thr-1', runId: 'run-9' }
  const base = { waiting, busy: false, observed: true, threadLive: true }

  it('goes out on the run\'s done event, even while the projection still lists the thread as working', () => {
    expect(deferredSendReady({ ...base, run: 'done', threadLive: true })).toBe(true)
  })

  it('does not go out when the projection shows the thread idle but the run has not said it is done', () => {
    expect(deferredSendReady({ ...base, run: 'running', threadLive: false })).toBe(false)
  })

  it('waits for this composer\'s own send to settle, whatever the run says', () => {
    expect(deferredSendReady({ ...base, run: 'done', busy: true })).toBe(false)
  })

  it('falls back to an observed, idle projection only when no done event can arrive', () => {
    expect(deferredSendReady({ ...base, run: 'unknown', threadLive: false })).toBe(true)
    expect(deferredSendReady({ ...base, run: 'unknown', threadLive: true })).toBe(false)
    expect(deferredSendReady({ ...base, run: 'unknown', threadLive: false, observed: false })).toBe(false)
  })

  it('has nothing to send when nothing waits', () => {
    expect(deferredSendReady({ ...base, waiting: undefined, run: 'done' })).toBe(false)
  })
})
