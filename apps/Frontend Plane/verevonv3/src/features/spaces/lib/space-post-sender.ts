import { createSignal } from 'solid-js'

import { ApiError } from '@/shared/api/http'
import {
  createSpacePost,
  replyInSpaceThread,
  sameMentions,
  type SpaceMentionsInput,
  type SpacePostReceipt,
} from '@/shared/api/spaces-client'
import { noteSendTransportLost, noteSendTransportOk } from './space-connectivity'
import type { SpaceJournalEntry, SpaceJournalFailure, SpaceSendJournal } from './space-send-journal'
import { newSpaceSendId } from './space-send-pipeline'

/**
 * The human-post send path (WS01-S6): "Send" in the room composer. It is the `post` mode's
 * transport, beside the pipeline's streamed `ask` mode, and it never starts a model run: the
 * gateway stores the text as the member's own post under a decision Control signs per send.
 *
 * Reliability is the journal's: one `sendId` per user action, reused by every retry, so a send
 * whose answer was lost comes back `deduplicated` instead of posting twice. Offline, the send is
 * `queued` and goes out once when the browser says `online`. A failure keeps the text in the box
 * (the composer clears it only on acceptance) and offers Retry and Discard.
 *
 * Exactly one unsettled post per composer: `busy` (queued or sending) blocks another send.
 */

/** How long an accepted entry may outlive its acceptance if the room's projection never shows it. */
const ACCEPTED_TTL_MS = 30_000

/** Codes where repeating the same send under its own key can succeed. */
const RETRYABLE_CODES: ReadonlySet<string> = new Set([
  'space_decision_unavailable',
  'space_lifecycle_unavailable',
  'delegated_auth_unavailable',
  'session_unavailable',
  'post_not_stored',
  'space_authority_stale',
  'zdr_decision_unavailable',
])

export interface SpacePostInput {
  /** Already trimmed and checked against the post bound by the caller. */
  readonly content: string
  /** Set when the post continues an existing room thread. */
  readonly replyThreadId?: string
  /** WS08: ids picked in the box (people and agents). Omitted for a plain post. */
  readonly mentions?: SpaceMentionsInput
}

export interface SpacePostSenderOptions {
  readonly journal: SpaceSendJournal
  /** Read at call time; the composer's room can change under it. */
  readonly spaceRef: () => string
  /** The room's own words for a failed send (known codes translated, unknown kept generic). */
  readonly describeError: (error: unknown) => string
  /** The reply thread's current `updated_at`, so the journal can tell when the room shows the reply. */
  readonly baselineUpdatedAt?: (threadId: string) => string | undefined
  /** The server stored the post. The composer clears the box and re-reads the room here. */
  readonly onAccepted?: (entry: SpaceJournalEntry, receipt: SpacePostReceipt) => void
  /** The server refused durable writes for this organization (typed zero-retention refusal). */
  readonly onZdrRefused?: () => void
  /** Defaults to the browser's own `navigator.onLine`. */
  readonly isOnline?: () => boolean
}

export interface SpacePostSender {
  /** This composer's unsettled post (queued, sending or failed); undefined once accepted or discarded. */
  readonly current: () => SpaceJournalEntry | undefined
  /** Queued or sending: another send must wait. */
  readonly busy: () => boolean
  readonly send: (input: SpacePostInput) => Promise<void>
  /** Repeat a failed send under its own `sendId`. */
  readonly retry: () => Promise<void>
  /** Drop the unsettled post. The text stays in the box; nothing is sent. */
  readonly discard: () => void
  /** Stop listening for the browser coming back online. */
  readonly dispose: () => void
}

function browserOnline(): boolean {
  return typeof navigator === 'undefined' || navigator.onLine !== false
}

function classify(error: unknown, describe: (error: unknown) => string): SpaceJournalFailure {
  if (error instanceof ApiError) {
    const code = error.code ?? `http_${error.status}`
    return {
      code,
      message: describe(error),
      retryable: error.status >= 500 || (error.code !== null && RETRYABLE_CODES.has(error.code)),
    }
  }
  // No answer at all (a dropped connection, a service restart): the outcome is unknown, and a
  // retry under the same key is exactly what makes that safe.
  return { code: 'network', message: describe(error), retryable: true }
}

export function createSpacePostSender(options: SpacePostSenderOptions): SpacePostSender {
  const { journal } = options
  const [currentId, setCurrentId] = createSignal<string | undefined>(undefined)
  const online = () => (options.isOnline ?? browserOnline)()

  const current = (): SpaceJournalEntry | undefined => {
    const id = currentId()
    const entry = id ? journal.get(id) : undefined
    return entry && entry.stage !== 'accepted' ? entry : undefined
  }
  const busy = () => {
    const stage = current()?.stage
    return stage === 'queued' || stage === 'sending'
  }

  // Takes the entry itself: Solid 2 applies a signal write at the next flush, so `journal.get`
  // straight after `journal.begin` still sees the old list and the first send would never go out.
  // Reads after the request (an await) are safe: the writes have been flushed by then.
  async function attempt(entry: SpaceJournalEntry): Promise<void> {
    const sendId = entry.sendId
    if (!online()) {
      journal.update(sendId, { stage: 'queued' })
      return
    }
    journal.update(sendId, { stage: 'sending' })
    try {
      const input = { content: entry.content, clientSendId: sendId, mentions: entry.mentions }
      const receipt = entry.kind === 'post_reply' && entry.threadId
        ? await replyInSpaceThread(entry.spaceRef, entry.threadId, input)
        : await createSpacePost(entry.spaceRef, input)
      // Discarded while the request was out: the server may well have stored it (the key makes
      // a later identical send safe), but this composer no longer shows it.
      if (!journal.get(sendId)) return
      noteSendTransportOk()
      journal.update(sendId, { stage: 'accepted', threadId: receipt.threadId })
      setTimeout(() => {
        if (journal.get(sendId)?.stage === 'accepted') journal.remove(sendId)
      }, ACCEPTED_TTL_MS)
      options.onAccepted?.({ ...entry, stage: 'accepted', threadId: receipt.threadId, failure: undefined }, receipt)
    } catch (error) {
      if (!journal.get(sendId)) return
      const failure = classify(error, options.describeError)
      if (error instanceof ApiError && error.code === 'zdr_durable_mutation_forbidden') options.onZdrRefused?.()
      if (failure.code === 'network') noteSendTransportLost()
      journal.update(sendId, { stage: 'failed', failure })
    }
  }

  async function send(input: SpacePostInput): Promise<void> {
    if (busy()) return
    const content = input.content.trim()
    if (!content) return
    const spaceRef = options.spaceRef()
    const kind = input.replyThreadId ? 'post_reply' : 'post_new'
    const prior = current()
    // The same text to the same place after a failure is the same send: reuse its id so the room
    // gets it once. Different text is a different post, and the server refuses one key with two
    // bodies, so it gets its own id and the stale entry goes.
    const sameSend = prior?.stage === 'failed'
      && prior.kind === kind
      && prior.spaceRef === spaceRef
      && prior.content === content
      && sameMentions(prior.mentions, input.mentions)
      && (kind === 'post_new' || prior.threadId === input.replyThreadId)
    if (prior && !sameSend) journal.remove(prior.sendId)
    const entry = sameSend && prior
      ? prior
      : journal.begin({
          sendId: newSpaceSendId(),
          kind,
          spaceRef,
          threadId: input.replyThreadId,
          content,
          ...(input.mentions ? { mentions: input.mentions } : {}),
          baselineUpdatedAt: input.replyThreadId ? options.baselineUpdatedAt?.(input.replyThreadId) : undefined,
          stage: online() ? 'sending' : 'queued',
        })
    setCurrentId(entry.sendId)
    await attempt(entry)
  }

  async function retry(): Promise<void> {
    const entry = current()
    if (!entry || entry.stage !== 'failed' || entry.failure?.retryable !== true) return
    await attempt(entry)
  }

  function discard(): void {
    const entry = current()
    if (!entry || entry.stage === 'sending') return
    journal.remove(entry.sendId)
    setCurrentId(undefined)
  }

  // A queued post goes out once when the browser is back.
  const onOnline = () => {
    const entry = current()
    if (entry?.stage === 'queued') void attempt(entry)
  }
  if (typeof window !== 'undefined') window.addEventListener('online', onOnline)
  const dispose = () => {
    if (typeof window !== 'undefined') window.removeEventListener('online', onOnline)
  }

  return { current, busy, send, retry, discard, dispose }
}
