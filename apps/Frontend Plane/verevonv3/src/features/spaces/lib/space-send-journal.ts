import { createSignal } from 'solid-js'

import type { SpaceMentionsInput } from '@/shared/api/spaces-client'

/**
 * The send journal (WS01-S6, F-t1-033): what this browser has sent to a room and not yet seen
 * come back in the room's own record. One entry per user action, named by its `sendId`, with
 * the stages a person can understand:
 *
 * - `queued`   held because the browser is offline; it sends when the browser is back;
 * - `sending`  the request is on its way;
 * - `accepted` the server stored it (the receipt names the thread and the message); the entry stays
 *              only until the room shows it, so the optimistic row never doubles the real one: once the
 *              room's record carries its message id (a new post's row sits in its listed thread until
 *              then); a receipt that named no message id falls back to the listing (a new post when its
 *              thread is listed, a reply when its thread's `updated_at` moves past the one it was sent against);
 * - `failed`   it did not land; the entry keeps the text, says why, and a retry reuses the SAME
 *              `sendId`, so a send whose answer was lost lands once.
 *
 * In memory only. A draft that survives a reload is WS15's, and writing a post's text to browser
 * storage would need the zero-retention rules that belong with it. Never an authorization input:
 * an entry is a presentation of what the member did, not proof that anything was stored.
 *
 * `agent_turn` names the agent path in the vocabulary; its progress is the composer's exchange
 * block (the stream), so it is not recorded here.
 */
export type SpaceJournalKind = 'post_new' | 'post_reply' | 'agent_turn'

export type SpaceJournalStage = 'queued' | 'sending' | 'accepted' | 'failed'

export interface SpaceJournalFailure {
  /** The gateway's error code, or `network` when no answer arrived. */
  readonly code: string
  /** The room's own words for it, already translated. */
  readonly message: string
  /** True when repeating the send under its own key can succeed (a transport or service failure). */
  readonly retryable: boolean
}

export interface SpaceJournalEntry {
  readonly sendId: string
  readonly kind: SpaceJournalKind
  readonly spaceRef: string
  /** The thread a reply goes to; for a new post, the thread the receipt named once accepted. */
  readonly threadId?: string
  /**
   * The server's id for the stored post (the receipt's `message_id`), once accepted. The room's record names a
   * message by it, so an accepted reply is swapped for its real turn by this id, never by a timestamp.
   */
  readonly messageId?: string
  readonly content: string
  readonly stage: SpaceJournalStage
  readonly failure?: SpaceJournalFailure
  readonly createdAt: number
  /**
   * A reply's thread `updated_at` when it was sent. Only the fallback for a receipt that named no message id: there,
   * the projection moving past it means "it is in the record".
   */
  readonly baselineUpdatedAt?: string
  /** WS08: the ids picked in the box. Kept so a retry under the same key sends the same body. */
  readonly mentions?: SpaceMentionsInput
}

export type SpaceJournalStart = Pick<
  SpaceJournalEntry,
  'sendId' | 'kind' | 'spaceRef' | 'threadId' | 'content' | 'baselineUpdatedAt' | 'mentions'
> & { readonly stage?: SpaceJournalStage }

export interface SpaceSendJournal {
  readonly entries: () => readonly SpaceJournalEntry[]
  readonly forSpace: (spaceRef: string) => readonly SpaceJournalEntry[]
  readonly get: (sendId: string) => SpaceJournalEntry | undefined
  /** Record a new send (stage `sending` unless given). Replaces an entry with the same id. */
  readonly begin: (start: SpaceJournalStart) => SpaceJournalEntry
  readonly update: (sendId: string, patch: Partial<Omit<SpaceJournalEntry, 'sendId'>>) => void
  readonly remove: (sendId: string) => void
  /**
   * Drop accepted entries the room's own listing shows, for receipts that named no message id only: a new post whose
   * thread is listed, a reply whose thread's `updated_at` moved past the one it was sent against. An entry named by its
   * message id (a new post or a reply) waits for `recorded`, so it is swapped for its real turn by id.
   */
  readonly reconcile: (
    spaceRef: string,
    threads: readonly { readonly thread_id: string; readonly updated_at?: string }[],
  ) => void
  /**
   * Drop accepted entries whose stored message the room's record now carries: the message ids of the turns this room
   * has read. The swap is by id, so a message the room shows is never drawn twice.
   */
  readonly recorded: (spaceRef: string, messageIds: readonly string[]) => void
}

/**
 * Whether a thread's `updated_at` has moved forward past the baseline a reply was sent against, so the reply is in
 * the record. Forward, as instants: a listing that answers with an older stamp, or with none, does not show the reply
 * yet, and an unparsable stamp is no evidence either way.
 */
function movedForward(updatedAt: string | undefined, baseline: string | undefined): boolean {
  const now = Date.parse(updatedAt ?? '')
  if (Number.isNaN(now)) return false
  const then = Date.parse(baseline ?? '')
  return Number.isNaN(then) || now > then
}

export function createSpaceSendJournal(): SpaceSendJournal {
  const [entries, setEntries] = createSignal<readonly SpaceJournalEntry[]>([])
  const forSpace = (spaceRef: string) => entries().filter((entry) => entry.spaceRef === spaceRef)
  const get = (sendId: string) => entries().find((entry) => entry.sendId === sendId)

  function begin(start: SpaceJournalStart): SpaceJournalEntry {
    const entry: SpaceJournalEntry = {
      sendId: start.sendId,
      kind: start.kind,
      spaceRef: start.spaceRef,
      ...(start.threadId ? { threadId: start.threadId } : {}),
      content: start.content,
      stage: start.stage ?? 'sending',
      createdAt: Date.now(),
      ...(start.baselineUpdatedAt ? { baselineUpdatedAt: start.baselineUpdatedAt } : {}),
      ...(start.mentions ? { mentions: start.mentions } : {}),
    }
    setEntries((current) => [...current.filter((item) => item.sendId !== entry.sendId), entry])
    return entry
  }

  function update(sendId: string, patch: Partial<Omit<SpaceJournalEntry, 'sendId'>>): void {
    setEntries((current) => current.map((entry) => {
      if (entry.sendId !== sendId) return entry
      const next: SpaceJournalEntry = { ...entry, ...patch }
      // A failure belongs to the failed stage only: a retry or an acceptance clears it.
      return next.stage === 'failed' ? next : { ...next, failure: undefined }
    }))
  }

  function remove(sendId: string): void {
    setEntries((current) => (current.some((entry) => entry.sendId === sendId)
      ? current.filter((entry) => entry.sendId !== sendId)
      : current))
  }

  function reconcile(
    spaceRef: string,
    threads: readonly { readonly thread_id: string; readonly updated_at?: string }[],
  ): void {
    const byId = new Map(threads.map((thread) => [thread.thread_id, thread]))
    setEntries((current) => {
      const next = current.filter((entry) => {
        if (entry.spaceRef !== spaceRef || entry.stage !== 'accepted' || !entry.threadId) return true
        const thread = byId.get(entry.threadId)
        if (!thread) return true
        // A post the receipt named by its message id is settled by the record carrying that id, not by the listing.
        if (entry.messageId) return true
        if (entry.kind === 'post_new') return false
        return !movedForward(thread.updated_at, entry.baselineUpdatedAt)
      })
      return next.length === current.length ? current : next
    })
  }

  function recorded(spaceRef: string, messageIds: readonly string[]): void {
    if (messageIds.length === 0) return
    const shown = new Set(messageIds)
    setEntries((current) => {
      const next = current.filter((entry) => !(
        entry.spaceRef === spaceRef
        && entry.stage === 'accepted'
        && entry.messageId !== undefined
        && shown.has(entry.messageId)
      ))
      return next.length === current.length ? current : next
    })
  }

  return { entries, forSpace, get, begin, update, remove, reconcile, recorded }
}
