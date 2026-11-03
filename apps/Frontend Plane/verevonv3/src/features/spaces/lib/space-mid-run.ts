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
 * A steered message is in one of four states, over the three outcomes of `queueInvocationInput`:
 *
 * - `pending`   accepted by the gateway (`queued`), not yet handed to the agent;
 * - `delivered` the agent has it (a `queued_input` frame named it), whichever arrived first;
 * - `resent`    the run ended before taking it (`run_ended`, or an accepted message the run never
 *               read). It goes out again as an ordinary turn under the SAME client message id, which
 *               makes the transcript hold it once. That happens only after a clean finish;
 * - `refused`   not accepted, or accepted but never read by a turn that stopped or failed. The note
 *               says why. Never silent.
 *
 * `deferred` is the fifth thing, outside the strip: a message waiting for the agent to be free
 * ("send when ready"), kept in this browser until the thread is idle.
 *
 * Every read-then-write is a functional update. Solid 2 answers a plain read outside a reactive
 * scope with the last committed value, so a read right after a write would miss that write.
 */

/** Where a steer ended up, for the caller (the composer clears the box on anything but `refused`). */
export type SpaceSteerOutcome = 'queued' | 'resend' | 'refused'

/** One message that must go out as an ordinary turn once the run has finished cleanly. */
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

/** The gateway call a steer makes. The composer uses `queueInvocationInput`; tests pass a fake. */
export type SpaceQueueCall = (
  requestId: string,
  content: string,
  options: { threadId?: string; spaceRef?: string; clientMessageId?: string },
) => Promise<QueuedInputResult>

export interface SpaceMidRun {
  /** The strip entries for the turn in flight (empty when none). */
  readonly entries: () => readonly QueuedInput[]
  /** Hand a message to the running turn. */
  readonly steer: (content: string, target: SpaceSteerTarget) => Promise<SpaceSteerOutcome>
  /** A `queued_input` frame arrived: promote what it names to delivered. */
  readonly markDelivered: (messages: readonly string[], ids?: readonly string[]) => void
  /**
   * The turn finished cleanly. Messages the run never took are marked `resent` and returned, in the
   * order they were sent; the caller sends them one at a time, as ordinary turns.
   */
  readonly settle: () => readonly SpaceMidRunResend[]
  /**
   * The turn ended without a clean finish (stopped or failed): nothing is sent again. Every message the
   * run never took, and each of `unsent` (found by `settle` but not sent yet), is marked `refused`.
   */
  readonly abandon: (unsent: readonly SpaceMidRunResend[]) => void
  /** A new turn owns the strip: clear what the last run showed (a resend turn keeps its `resent` entries). */
  readonly startTurn: (resendOfQueued: boolean) => void
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

const NOT_STARTED_NOTE = {
  no: 'Kjøringen har ikke startet ennå. Prøv igjen om et øyeblikk.',
  en: 'The run has not started yet. Try again in a moment.',
} as const

const NOT_SAVED_NOTE = {
  no: 'Levert, men ikke lagret i samtalehistorikken.',
  en: 'Delivered, but not saved in the conversation history.',
} as const

const NOT_SENT_AGAIN_NOTE = {
  no: 'Kjøringen stoppet før agenten leste meldingen. Den ble ikke sendt på nytt.',
  en: 'The run stopped before the agent read it. It was not sent again.',
} as const

/**
 * `mint` names a message. `settled` says whether the turn has already ended: a steer answered after
 * that has nobody left to settle it, so it is handed back to the caller at once. `tr` picks the
 * interface language.
 */
export function createSpaceMidRun(options: {
  readonly mint?: () => string
  readonly settled?: () => boolean
  readonly tr: (no: string, en: string) => string
  readonly queue?: SpaceQueueCall
}): SpaceMidRun {
  const queue: SpaceQueueCall = options.queue ?? queueInvocationInput
  const mint = options.mint ?? (() => `msg-${Date.now()}-${Math.random().toString(36).slice(2, 10)}`)
  const [entries, setEntries] = createSignal<readonly QueuedInput[]>([])
  const [deferred, setDeferred] = createSignal<SpaceDeferredSend | undefined>(undefined)
  const [delivering, setDelivering] = createSignal(0)
  // Messages the run ended without taking, to be sent again once the turn finishes cleanly. Plain
  // state rather than a signal: only `steer` and `settle` touch it, and both run in the same tick as
  // the turn's own transitions.
  let held: SpaceMidRunResend[] = []

  const patch = (id: string, change: Partial<QueuedInput>) =>
    setEntries((current) => current.map((entry) => (entry.id === id ? { ...entry, ...change } : entry)))

  async function ask(
    requestId: string,
    content: string,
    target: SpaceSteerTarget,
    clientMessageId: string,
  ): Promise<QueuedInputResult> {
    setDelivering((count) => count + 1)
    try {
      return await queue(requestId, content, {
        threadId: target.threadId,
        spaceRef: target.spaceRef,
        clientMessageId,
      })
    } catch (error) {
      // A request that fails before the gateway answers is a refusal: nothing was accepted.
      return { outcome: 'refused', reason: 'other', message: error instanceof Error ? error.message : String(error) }
    } finally {
      setDelivering((count) => count - 1)
    }
  }

  async function steer(content: string, target: SpaceSteerTarget): Promise<SpaceSteerOutcome> {
    const id = `queued-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`
    const clientMessageId = mint()
    setEntries((current) => [...current, { id, content, state: 'pending', clientMessageId }])
    const requestId = target.requestId?.trim()
    if (!requestId) {
      // The stream is still opening: there is no run to hand it to yet. Say so and let the caller
      // keep the words; nothing was sent.
      patch(id, { state: 'refused', note: options.tr(NOT_STARTED_NOTE.no, NOT_STARTED_NOTE.en) })
      return 'refused'
    }
    const result = await ask(requestId, content, target, clientMessageId)
    // Read the turn's state after the answer: it may have ended while the call was in the air.
    const ended = options.settled?.() === true

    if (result.outcome === 'queued') {
      // A `queued_input` frame can name the message before this answer arrives. A delivered message
      // stays delivered and is never sent again; a pending one is held if the turn has already ended.
      const decision = { resend: false }
      const note = result.persisted ? undefined : options.tr(NOT_SAVED_NOTE.no, NOT_SAVED_NOTE.en)
      setEntries((current) => current.map((entry): QueuedInput => {
        if (entry.id !== id) return entry
        if (entry.state !== 'pending') return { ...entry, accepted: true }
        if (ended) {
          decision.resend = true
          return { ...entry, state: 'resent', accepted: false, note: undefined }
        }
        return { ...entry, accepted: true, note }
      }))
      if (decision.resend) {
        held = [...held, { content, clientMessageId }]
        return 'resend'
      }
      return 'queued'
    }

    if (result.outcome === 'run_ended') {
      // The stream ended before the message landed. Not an error: it goes out again as an ordinary
      // turn, under the id the gateway stored it with, once the turn finishes cleanly.
      const again = result.clientMessageId ?? clientMessageId
      const decision = { resend: false }
      setEntries((current) => current.map((entry): QueuedInput => {
        if (entry.id !== id || entry.state !== 'pending') return entry
        decision.resend = true
        return { ...entry, state: 'resent', accepted: false, note: options.tr(RUN_ENDED_NOTE.no, RUN_ENDED_NOTE.en) }
      }))
      if (!decision.resend) return 'queued'
      held = [...held, { content, clientMessageId: again }]
      return 'resend'
    }

    patch(id, { state: 'refused', accepted: false, note: result.message })
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
      const settledNow = { resend: [] as QueuedInput[] }
      setEntries((current) => {
        const settled = settleQueuedInputsAtRunEnd(current)
        settledNow.resend = settled.resend
        return settled.entries
      })
      const again = held
      held = []
      return [...settledNow.resend.map(toResend), ...again]
    },
    abandon: (unsent) => {
      const named = new Set<string>()
      for (const item of [...unsent, ...held]) {
        if (item.clientMessageId) named.add(item.clientMessageId)
      }
      held = []
      setEntries((current) => current.map((entry): QueuedInput => {
        // Accepted, and never read: the turn ended before the run took it.
        const unread = entry.state === 'pending' && entry.accepted === true
        // Held for a resend that will not happen now.
        const withheld = entry.state === 'resent' && entry.clientMessageId !== undefined && named.has(entry.clientMessageId)
        if (!unread && !withheld) return entry
        return { ...entry, state: 'refused', accepted: false, note: options.tr(NOT_SENT_AGAIN_NOTE.no, NOT_SENT_AGAIN_NOTE.en) }
      }))
    },
    startTurn: (resendOfQueued) =>
      setEntries((current) => (current.length > 0 ? queuedInputsForNewTurn(current, resendOfQueued) : current)),
    deferred,
    defer: (message) => setDeferred(message),
    takeDeferred: () => {
      const taken: { message?: SpaceDeferredSend } = {}
      setDeferred((current) => {
        taken.message = current
        return undefined
      })
      return taken.message
    },
    cancelDeferred: () => setDeferred(undefined),
    delivering: () => delivering() > 0,
  }
}
