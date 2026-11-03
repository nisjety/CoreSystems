import { createSignal } from 'solid-js'

import { executeAction } from '@/shared/actions/action-client'
import { browserRunSteerOutput } from '@/shared/actions/action-registry'
import type { ActionActor, ActionExecution } from '@/shared/actions/types'
import { ApiError } from '@/shared/api/api-error'

// Steer and queue for a live browser-agent run in the dock (E3.3-g). While the run works, a message
// can go two ways:
//
// - steer: `browser_run.steer` hands it to the owner, which the run takes at its next step. The owner
//   answers `queued`, and the entry is `queued`. `markDelivered` makes it `delivered` once the run
//   reports the step. If the run ends first (`run_ended`), nothing starts: the entry becomes
//   `not_taken`, and the text stays in front of the person, who decides what happens to it.
// - queue: the message is held in this browser only. `settle()` hands the held messages back when the
//   run has ended, and the page sends them as ordinary messages. Nothing reaches the owner for them.
//
// Ephemeral: the text lives in this module's memory and nowhere else. It is not written to browser
// storage, the transcript or the run's event log, and a reload drops it. Zero data retention refuses
// both ways. The controller never starts a run: a new run is `browser_run.start`, which needs approval.

/** The most a steered or queued message may carry: the contract's own bound (`browser_run.steer`). */
export const STEER_MAX_CHARS = 2000

/** Messages the queue holds at once. A further one is refused, so the queue cannot grow without bound. */
export const STEER_HELD_MAX = 5

/**
 * Whether the steer composer is on for this build. `VITE_CHAT_BROWSER_STEER` counts only as "1" or
 * "true", so an unset variable is off: a build stays off until its environment turns the composer on.
 */
export function isChatBrowserSteerEnabled(): boolean {
  const value = import.meta.env.VITE_CHAT_BROWSER_STEER
  return value === '1' || value === 'true'
}

/** Why a message was not sent or not queued. The composer shows the matching sentence. */
export type SteerRefusal = 'empty' | 'too_long' | 'zdr' | 'not_live' | 'signed_out' | 'queue_full' | 'failed'

/**
 * Where one message stands. `sending`: the owner is answering. `queued`: the run has it for its next
 * step. `delivered`: the run reported taking it. `not_taken`: the run ended first. `held`: waiting in
 * the queue until the run ends. `refused`: not sent, with the reason in `refusal`.
 */
export type SteerState = 'sending' | 'queued' | 'delivered' | 'not_taken' | 'held' | 'refused'

export interface SteerEntry {
  /** Local to one controller; the strip keys on it. */
  readonly id: string
  readonly content: string
  /** The id the owner stores the message under. `markDelivered` matches on it. */
  readonly clientMessageId: string
  readonly state: SteerState
  /** Set on `refused` entries only. */
  readonly refusal?: SteerRefusal
}

/**
 * What a steer or a hold answered. The composer clears its box on `queued` and `held`. On `run_ended`
 * and `refused` it keeps the text, so nothing the person wrote is lost.
 */
export type SteerOutcome =
  | { readonly outcome: 'queued' | 'held' | 'run_ended' }
  | { readonly outcome: 'refused'; readonly refusal: SteerRefusal }

/** A held message the page sends as an ordinary message once the run has ended. */
export interface SteerResend {
  readonly content: string
  readonly clientMessageId: string
}

export interface BrowserSteerOptions {
  /** The live run's id, read at send time so it is never stale. */
  readonly runId: () => string | undefined
  /** The run is working right now. */
  readonly live: () => boolean
  /** Zero Data Retention is on for this session. */
  readonly zdr: () => boolean
  /** The signed-in person in the active organisation. The owner checks it again. */
  readonly actor: () => ActionActor | undefined
  /** Mints a message id. Defaults to a random UUID. */
  readonly mint?: () => string
  /** Sends the action. Defaults to the shared action client. */
  readonly execute?: typeof executeAction
}

export interface BrowserSteer {
  readonly entries: () => readonly SteerEntry[]
  /** At least one steer is waiting for the owner's answer. */
  readonly sending: () => boolean
  /** Hands one message to the live run, to be taken at its next step. */
  readonly steer: (text: string) => Promise<SteerOutcome>
  /** Holds one message in this browser until the run has ended. Sends nothing. */
  readonly hold: (text: string) => SteerOutcome
  /** A step took these messages: the `queued` entries they name become `delivered`. */
  readonly markDelivered: (clientMessageIds: readonly string[]) => void
  /**
   * The run has ended. Steers it never took become `not_taken`. The held messages leave the strip and
   * are returned, oldest first, for the page to send as ordinary messages.
   */
  readonly settle: () => readonly SteerResend[]
  /** Removes one entry from the strip. */
  readonly dismiss: (id: string) => void
  /** A new run: clears the strip and re-arms the controller. */
  readonly reset: () => void
}

/** Trims a message and bounds it the way the contract does. */
export function normalizeSteerText(
  raw: string,
): { readonly ok: true; readonly content: string } | { readonly ok: false; readonly refusal: 'empty' | 'too_long' } {
  const content = raw.trim()
  if (content.length === 0) return { ok: false, refusal: 'empty' }
  if (content.length > STEER_MAX_CHARS) return { ok: false, refusal: 'too_long' }
  return { ok: true, content }
}

/** A failed send. The server names the feature of a zero-retention refusal; anything else reads as `failed`. */
function refusalOf(failure: unknown): SteerRefusal {
  return failure instanceof ApiError && failure.feature !== null ? 'zdr' : 'failed'
}

/**
 * The steer and queue controller for one browser-agent run. The page makes one per run surface, passes
 * it to `SteerInput`, calls `settle()` when the run ends and `reset()` when a new run starts.
 */
export function createBrowserSteer(options: BrowserSteerOptions): BrowserSteer {
  const execute = options.execute ?? executeAction
  const mint = options.mint ?? (() => crypto.randomUUID())
  const [entries, setEntries] = createSignal<readonly SteerEntry[]>([])
  const [inFlight, setInFlight] = createSignal(0)
  let counter = 0
  // Set when the run has ended. An answer that arrives after that is settled here, not left queued.
  let ended = false
  // The last send that failed. Sent again with the same text it keeps its id, so an owner that
  // dedupes on the id stores one message, not two.
  let retry: SteerResend | undefined

  const nextId = (): string => {
    counter += 1
    return `steer-${counter}`
  }
  const add = (entry: SteerEntry): void => setEntries((current) => [...current, entry])
  const change = (id: string, patch: Partial<SteerEntry>): void =>
    setEntries((current) => current.map((entry) => (entry.id === id ? { ...entry, ...patch } : entry)))
  const refuseNow = (content: string, refusal: SteerRefusal): SteerOutcome => {
    add({ id: nextId(), content, clientMessageId: mint(), state: 'refused', refusal })
    return { outcome: 'refused', refusal }
  }
  // Whether the run can take a message at all right now. The person's identity is checked by `steer`.
  const blocked = (): SteerRefusal | undefined => {
    if (options.zdr()) return 'zdr'
    if (!options.live()) return 'not_live'
    return undefined
  }

  async function steer(text: string): Promise<SteerOutcome> {
    const checked = normalizeSteerText(text)
    if (!checked.ok) return { outcome: 'refused', refusal: checked.refusal }
    const refusal = blocked()
    if (refusal) return refuseNow(checked.content, refusal)
    const runId = options.runId()
    if (!runId) return refuseNow(checked.content, 'not_live')
    const actor = options.actor()
    if (!actor) return refuseNow(checked.content, 'signed_out')

    const clientMessageId = retry?.content === checked.content ? retry.clientMessageId : mint()
    const id = nextId()
    add({ id, content: checked.content, clientMessageId, state: 'sending' })
    setInFlight((count) => count + 1)
    let execution: ActionExecution | undefined
    let failed: SteerRefusal | undefined
    try {
      execution = await execute(
        'browser_run.steer',
        actor,
        { runId, content: checked.content, clientMessageId },
        { idempotencyKey: `browser-steer:${clientMessageId}` },
      )
    } catch (failure) {
      failed = refusalOf(failure)
    } finally {
      setInFlight((count) => count - 1)
    }
    const fail = (reason: SteerRefusal): SteerOutcome => {
      retry = reason === 'zdr' ? undefined : { content: checked.content, clientMessageId }
      change(id, { state: 'refused', refusal: reason })
      return { outcome: 'refused', refusal: reason }
    }
    if (failed !== undefined) return fail(failed)
    if (execution === undefined) return fail('failed')
    const answer = browserRunSteerOutput.safeParse(execution.result)
    if (!answer.success) return fail('failed')
    retry = undefined
    if (answer.data.outcome === 'queued' && !ended) {
      change(id, { state: 'queued' })
      return { outcome: 'queued' }
    }
    change(id, { state: 'not_taken' })
    return { outcome: 'run_ended' }
  }

  function hold(text: string): SteerOutcome {
    const checked = normalizeSteerText(text)
    if (!checked.ok) return { outcome: 'refused', refusal: checked.refusal }
    const refusal = blocked()
    if (refusal) return refuseNow(checked.content, refusal)
    if (entries().filter((entry) => entry.state === 'held').length >= STEER_HELD_MAX) {
      return refuseNow(checked.content, 'queue_full')
    }
    add({ id: nextId(), content: checked.content, clientMessageId: mint(), state: 'held' })
    return { outcome: 'held' }
  }

  function markDelivered(clientMessageIds: readonly string[]): void {
    setEntries((current) =>
      current.map((entry): SteerEntry =>
        entry.state === 'queued' && clientMessageIds.includes(entry.clientMessageId)
          ? { ...entry, state: 'delivered' }
          : entry,
      ),
    )
  }

  function settle(): readonly SteerResend[] {
    ended = true
    const handed = entries()
      .filter((entry) => entry.state === 'held')
      .map((entry): SteerResend => ({ content: entry.content, clientMessageId: entry.clientMessageId }))
    setEntries((current) =>
      current
        .filter((entry) => entry.state !== 'held')
        .map((entry): SteerEntry => (entry.state === 'queued' ? { ...entry, state: 'not_taken' } : entry)),
    )
    return handed
  }

  function dismiss(id: string): void {
    setEntries((current) => current.filter((entry) => entry.id !== id))
  }

  function reset(): void {
    ended = false
    retry = undefined
    setEntries(() => [])
  }

  return {
    entries,
    sending: () => inFlight() > 0,
    steer,
    hold,
    markDelivered,
    settle,
    dismiss,
    reset,
  }
}
