import { createSignal } from 'solid-js'

/**
 * The send journal (WS01-S6, F-t1-033): what this browser has sent to a room and not yet seen
 * come back in the room's own record. One entry per user action, named by its `sendId`, with
 * the stages a person can understand:
 *
 * - `queued`   held because the browser is offline; it sends when the browser is back;
 * - `sending`  the request is on its way;
 * - `accepted` the server stored it (the receipt names the thread); the entry stays only until
 *              the room's own projection shows it, so the optimistic row never doubles the real one;
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
  readonly content: string
  readonly stage: SpaceJournalStage
  readonly failure?: SpaceJournalFailure
  readonly createdAt: number
  /** A reply's thread `updated_at` when it was sent, so the projection moving past it means "it is in the record". */
  readonly baselineUpdatedAt?: string
}

export type SpaceJournalStart = Pick<
  SpaceJournalEntry,
  'sendId' | 'kind' | 'spaceRef' | 'threadId' | 'content' | 'baselineUpdatedAt'
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
   * Drop accepted entries the room's own projection now shows: a new post whose thread is listed,
   * a reply whose thread's `updated_at` moved past the one it was sent against.
   */
  readonly reconcile: (
    spaceRef: string,
    threads: readonly { readonly thread_id: string; readonly updated_at?: string }[],
  ) => void
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
        if (entry.kind === 'post_new') return false
        return (thread.updated_at ?? '') === (entry.baselineUpdatedAt ?? '')
      })
      return next.length === current.length ? current : next
    })
  }

  return { entries, forSpace, get, begin, update, remove, reconcile }
}
