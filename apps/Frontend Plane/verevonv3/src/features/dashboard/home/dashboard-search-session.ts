import { createSignal, untrack } from 'solid-js'
import { removeClientValue } from '@/shared/session/client-storage'
import { onSearchSnapshotsCleared, searchSnapshotStorageKey } from '@/shared/session/home-local-content'
import { getSession } from '@/shared/session/session-store'

// Search session state lives at module scope so the active query survives the
// panel unmounting on a tab switch — combined with the 4h web-search cache, the
// last query is restored (and its preview re-renders instantly from cache) when
// you return to the Søk tab.
export const [query, setQuery] = createSignal('')

/**
 * Where the saved snapshot of the signed-in user in the current workspace lives,
 * or null until both are known. Read when a snapshot is read, written or removed,
 * not when a panel was created: the person or workspace may have changed since.
 */
export function currentSearchSnapshotKey(): string | null {
  return untrack(() => {
    const session = getSession()
    return searchSnapshotStorageKey(session.user?.id, session.activeOrg?.id)
  })
}

/**
 * Forgets the shared query and removes the saved snapshot. The caller remounts
 * SearchPanel (Home re-keys it): disposing the old panel aborts whatever it has
 * in flight, and the new one reads the now-empty snapshot and starts clean. The
 * 4h web-search cache lives in the query client and is left alone, so repeating
 * an earlier query is still instant.
 */
export function resetSearchSession(): void {
  setQuery('')
  const key = currentSearchSnapshotKey()
  if (key) removeClientValue(key)
}

// Sign-out, a different user and a zero-retention verdict remove every saved
// search from the browser (shared/session/home-local-content, which cannot reach
// this module). The query kept in memory has to go with them, or the next person
// in this tab would find it in the box.
onSearchSnapshotsCleared(() => setQuery(''))

// The mounted SearchPanel offers one way in from outside it: run this search as
// if it had been typed and submitted (the Nett history card re-runs an earlier
// search this way). There is no queue: with no panel mounted nothing is pending
// and nothing runs later, so a stale request can never fire on a future mount.
type SearchRunOptions = {
  /**
   * The search was started from the keyboard, and the control that started it goes
   * away when the results open: the results page's search box takes focus.
   */
  focus?: boolean
}
type SearchRunner = (text: string, options?: SearchRunOptions) => void
let searchRunner: SearchRunner | undefined

/** The panel calls this while mounted; the returned function withdraws it (only if it is still the current one). */
export function registerSearchRunner(runner: SearchRunner): () => void {
  searchRunner = runner
  return () => {
    if (searchRunner === runner) searchRunner = undefined
  }
}

/**
 * Runs `text` through the search panel's own submit flow. Without a mounted
 * panel it only puts the text in the shared query, and returns false so the
 * caller can send the person to the search box instead.
 */
export function runSearch(text: string, options?: SearchRunOptions): boolean {
  const trimmed = text.trim()
  if (!trimmed) return false
  if (searchRunner) {
    searchRunner(trimmed, options)
    return true
  }
  setQuery(trimmed)
  return false
}
