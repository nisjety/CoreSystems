import { createSignal } from 'solid-js'

/**
 * A per-thread "read the transcript again" counter (DEPTH-WS12-S4-1).
 *
 * An edit, a delete or a moderator removal changes a thread's text without a new message, so the
 * thread's `updated_at` and `last_message_seq` (the revision every transcript read is keyed on) do
 * not move. Session Core signals the change on the Space stream as a content-free `thread.updated`
 * with the closed word `preview`; the stream controller hands those thread ids here, and every
 * transcript key mixes the counter in, so the open room re-reads exactly the threads that changed.
 *
 * Presentation only: it never decides what anyone may read (each re-read goes through the
 * transcript route and its Control decision).
 */
const [epochs, setEpochs] = createSignal<ReadonlyMap<string, number>>(new Map())

/** The counter of one thread; 0 until the first signal. Tracked: a key that reads it re-keys. */
export function threadTranscriptEpoch(threadId: string): number {
  return epochs().get(threadId) ?? 0
}

/** What the listing shows of one thread's text: the folded title and preview (DEPTH-WS12-S4-2). */
type ListedText = { thread_id: string; title?: string; preview?: string }

/**
 * Poll-mode fallback of the live signal: the threads whose listed title or preview changed since
 * the previous listing, and nothing on the first listing. Session Core folds a revision into the
 * title and preview, so an edit or delete of the first or last line shows here even when no
 * stream frame arrived (stream down or the event log switched off). A revision of a line in the
 * middle of a post still waits for the stream or a reload: the listing carries no revision mark.
 * `previous` is updated in place to the current listing.
 */
export function listedTextChanges(previous: Map<string, string>, threads: readonly ListedText[]): string[] {
  const changed: string[] = []
  const seen = new Set<string>()
  for (const thread of threads) {
    const text = `${thread.title ?? ''}\u0000${thread.preview ?? ''}`
    const before = previous.get(thread.thread_id)
    if (before !== undefined && before !== text) changed.push(thread.thread_id)
    previous.set(thread.thread_id, text)
    seen.add(thread.thread_id)
  }
  for (const id of [...previous.keys()]) if (!seen.has(id)) previous.delete(id)
  return changed
}

/** Bumps the counter of each named thread once. */
export function bumpThreadTranscriptEpochs(threadIds: Iterable<string>): void {
  const ids = [...new Set(threadIds)].filter((id) => id.length > 0)
  if (ids.length === 0) return
  setEpochs((previous) => {
    const next = new Map(previous)
    for (const id of ids) next.set(id, (next.get(id) ?? 0) + 1)
    return next
  })
}
