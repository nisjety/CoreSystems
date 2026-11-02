import { createSignal } from 'solid-js'

import {
  markQueuedInputsDelivered,
  queuedInputsForNewTurn,
  settleQueuedInputsAtRunEnd,
} from '@/features/chat/components/chat-normalizers'
import type { QueuedInput } from '@/features/chat/components/chat-types'
import { queueInvocationInput, type QueuedInputResult } from '@/shared/api/chat-client'
import type { Skill } from '@/shared/api/skills-client'

/**
 * Owner mid-run steer in the room composer (WS09-S2).
 *
 * The person who started a turn may type into it while it works. The message goes to the running
 * agent at its next tool-round boundary through the same queue route Chat uses
 * (`queueInvocationInput`); the room's `spaceRef` is only a hint, because the BFF strips every
 * authority field and mints a fresh Control append decision for this exact message. Nothing here is
 * authority, and nothing here is stored: the strip lives in memory, and a message that has to wait
 * for the agent to be free is kept in this browser only, and says so.
 *
 * Each message is in one of four states, over the three outcomes of `queueInvocationInput`:
 *
 * - `pending`   accepted by the gateway (`queued`), not yet handed to the agent;
 * - `delivered` the agent has it (a `queued_input` frame named it);
 * - `resent`    the run ended before taking it (`run_ended`, or an accepted message the run never
 *               read), so it goes out again as an ordinary turn under the SAME client message id,
 *               which makes the transcript hold it once;
 * - `refused`   not accepted, and the note says why. Never silent.
 *
 * `deferred` is the fifth thing, outside the strip: a message waiting for the agent to be free
 * ("send when ready"), kept in this browser until the thread is idle.
 */

/** Where a steer ended up, for the caller (the composer clears the box on anything but `refused`). */
export type SpaceSteerOutcome = 'queued' | 'resend' | 'refused'

/** One message that must go out as an ordinary turn once the run has ended. */
export interface SpaceMidRunResend {
  readonly content: string
  /** The id the message was named with, passed back as the send's own id so it lands once. */
  readonly clientMessageId?: string
}

/** The run a steer is aimed at. All three are hints for the gateway, never authority. */
export interface SpaceSteerTarget {
  readonly requestId: string | undefined
  readonly threadId: string | undefined
  readonly spaceRef: string
}

/** A message waiting for the agent to be free, kept in this browser only. */
export interface SpaceDeferredSend {
  readonly content: string
  readonly threadId: string
  /** What the member had chosen when they sent, so waiting does not change what is asked. */
  readonly mentionedAgentRef?: string
  readonly skills?: readonly Skill[]
}

export interface SpaceMidRun {
  /** The strip entries for the turn in flight (empty when none). */
  readonly entries: () => readonly QueuedInput[]
  /** Hand a message to the running turn. */
  readonly steer: (content: string, target: SpaceSteerTarget) => Promise<SpaceSteerOutcome>
  /** A `queued_input` frame arrived: promote what it names to delivered. */
  readonly markDelivered: (messages: readonly string[], ids?: readonly string[]) => void
  /**
   * The turn ended. Messages the run never took are marked `resent` and returned, to be sent as
   * ordinary turns; the caller sends them one at a time.
   */
  readonly settle: () => readonly SpaceMidRunResend[]
  /** A new turn owns the strip: clear what the last run showed (a resent turn keeps its marker). */
  readonly startTurn: (resendOfQueued: boolean) => void
  /** Take the resends that were found after the run had already settled (a late `run_ended`). */
  readonly takeLateResends: () => readonly SpaceMidRunResend[]
  readonly deferred: () => SpaceDeferredSend | undefined
  readonly defer: (message: SpaceDeferredSend) => void
  /** Returns the waiting message and forgets it, or undefined. */
  readonly takeDeferred: () => SpaceDeferredSend | undefined
  readonly cancelDeferred: () => void
  /** A steer call is in flight. */
  readonly delivering: () => boolean
}

const RUN_ENDED_NOTE = {
  no: 'Kjøringen ble ferdig – sendes som ny melding.',
  en: 'The run finished – sending it as a new message.',
} as const

/**
 * `mint` names a message; `settled` says whether the turn has already ended (a `run_ended` answer
 * that arrives after the stream closed has nobody left to flush it, so it is handed back through
 * `takeLateResends`). `tr` is the interface language picker.
 */
export function createSpaceMidRun(options: {
  readonly mint?: () => string
  readonly settled?: () => boolean
  readonly tr: (no: string, en: string) => string
  readonly queue?: typeof queueInvocationInput
}): SpaceMidRun {
  const queue = options.queue ?? queueInvocationInput
  const mint = options.mint ?? (() => `msg-${Date.now()}-${Math.random().toString(36).slice(2, 10)}`)
  const [entries, setEntries] = createSignal<readonly QueuedInput[]>([])
  const [deferred, setDeferred] = createSignal<SpaceDeferredSend | undefined>(undefined)
  const [delivering, setDelivering] = createSignal(0)
  let late: SpaceMidRunResend[] = []

  const update = (id: string, patch: Partial<QueuedInput>) =>
    setEntries((current) => current.map((entry) => (entry.id === id ? { ...entry, ...patch } : entry)))

  async function steer(content: string, target: SpaceSteerTarget): Promise<SpaceSteerOutcome> {
    const id = `queued-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`
    const clientMessageId = mint()
    setEntries((current) => [...current, { id, content, state: 'pending', clientMessageId }])
    const requestId = target.requestId?.trim()
    if (!requestId) {
      // The stream is still opening: there is no run to hand it to yet. Say so and let the caller
      // keep the words; nothing was sent.
      update(id, {
        state: 'refused',
        note: options.tr(
          'Kjøringen har ikke startet ennå. Prøv igjen om et øyeblikk.',
          'The run has not started yet. Try again in a moment.',
        ),
      })
      return 'refused'
    }
    setDelivering((count) => count + 1)
    let result: QueuedInputResult
    try {
      result = await queue(requestId, content, {
        threadId: target.threadId,
        spaceRef: target.spaceRef,
        clientMessageId,
      })
    } finally {
      setDelivering((count) => count - 1)
    }
    if (result.outcome === 'queued') {
      update(id, {
        state: 'pending',
        accepted: true,
        note: result.persisted ? undefined : options.tr(
          'Levert, men ikke lagret i samtalehistorikken.',
          'Delivered, but not saved in the conversation history.',
        ),
      })
      // The run may have ended while the call was in the air; settle() already ran with nothing to
      // resend, so an accepted message the run never took is found here.
      if (options.settled?.()) {
        const { entries: settledEntries, resend } = settleQueuedInputsAtRunEnd(entries())
        if (resend.length > 0) {
          setEntries(settledEntries)
          late = [...late, ...resend.map(toResend)]
          return 'resend'
        }
      }
      return 'queued'
    }
    if (result.outcome === 'run_ended') {
      update(id, { state: 'resent', accepted: false, note: options.tr(RUN_ENDED_NOTE.no, RUN_ENDED_NOTE.en) })
      late = [...late, { content, clientMessageId: result.clientMessageId ?? clientMessageId }]
      return 'resend'
    }
    update(id, { state: 'refused', note: result.message })
    return 'refused'
  }

  const toResend = (entry: QueuedInput): SpaceMidRunResend => ({
    content: entry.content,
    ...(entry.clientMessageId ? { clientMessageId: entry.clientMessageId } : {}),
  })

  return {
    entries,
    steer,
    markDelivered: (messages, ids = []) =>
      setEntries((current) => markQueuedInputsDelivered(current, messages, ids)),
    settle: () => {
      const { entries: settledEntries, resend } = settleQueuedInputsAtRunEnd(entries())
      if (resend.length > 0) setEntries(settledEntries)
      const taken = late
      late = []
      return [...resend.map(toResend), ...taken]
    },
    startTurn: (resendOfQueued) => setEntries((current) => (
      current.length > 0 ? queuedInputsForNewTurn(current, resendOfQueued) : current
    )),
    takeLateResends: () => {
      const taken = late
      late = []
      return taken
    },
    deferred,
    defer: (message) => setDeferred(message),
    takeDeferred: () => {
      const waiting = deferred()
      if (waiting) setDeferred(undefined)
      return waiting
    },
    cancelDeferred: () => setDeferred(undefined),
    delivering: () => delivering() > 0,
  }
}
