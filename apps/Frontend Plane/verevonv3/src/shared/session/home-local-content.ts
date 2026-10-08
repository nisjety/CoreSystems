import { removeClientValuesWithPrefix } from './client-storage'
import { clearHomeDrafts } from './home-draft-storage'

// What Home keeps on this device from things a person typed or looked up, next
// to the unsent draft (home-draft-storage): the Nett search snapshot and the
// Nett history of searches and opened pages. Both are keyed by workspace and
// user like the chat organizer, so one account never reads another's, and both
// are cleared here rather than where they are written, because the two places
// that clear them (sign-out and the workspace's zero-data-retention posture)
// sit in shared code that must not import a feature.
const SEARCH_SNAPSHOT_KEY_PREFIX = 'verevon.dashboard.search.session'
const WEB_HISTORY_KEY_PREFIX = 'verevon.home-web-history.v1'

function scopedKey(prefix: string, userId: string | undefined, orgId: string | undefined): string | null {
  const user = userId?.trim()
  const org = orgId?.trim()
  return user && org ? `${prefix}:${encodeURIComponent(org)}:${encodeURIComponent(user)}` : null
}

/** Where the Nett history of this user and workspace lives; null until both are known. */
export function webHistoryStorageKey(userId: string | undefined, orgId: string | undefined): string | null {
  return scopedKey(WEB_HISTORY_KEY_PREFIX, userId, orgId)
}

/** Where the Nett search snapshot of this user and workspace lives; null until both are known. */
export function searchSnapshotStorageKey(userId: string | undefined, orgId: string | undefined): string | null {
  return scopedKey(SEARCH_SNAPSHOT_KEY_PREFIX, userId, orgId)
}

const snapshotClearListeners = new Set<() => void>()

/**
 * Calls `listener` after every {@link clearSearchSnapshots}. The Nett query is
 * kept in memory by the search module, which cannot be reached from here, so it
 * asks to be told. Returns the function that withdraws the listener.
 */
export function onSearchSnapshotsCleared(listener: () => void): () => void {
  snapshotClearListeners.add(listener)
  return () => {
    snapshotClearListeners.delete(listener)
  }
}

/**
 * Removes every account's saved Nett search from this browser. The prefix has no
 * separator, so the unscoped key older builds wrote goes with them.
 */
export function clearSearchSnapshots(): void {
  removeClientValuesWithPrefix(SEARCH_SNAPSHOT_KEY_PREFIX)
  for (const listener of [...snapshotClearListeners]) {
    try {
      listener()
    } catch {
      // A listener that fails must not stop the others or the sign-out that called this.
    }
  }
}

/** Removes every account's Nett history from this browser. */
export function clearWebHistories(): void {
  removeClientValuesWithPrefix(`${WEB_HISTORY_KEY_PREFIX}:`)
}

/**
 * A workspace that is zero-data-retention allows no local copy of what a person
 * typed or searched for: drafts, saved searches and the Nett history all go.
 */
export function clearHomeLocalContent(): void {
  clearHomeDrafts()
  clearWebHistories()
  clearSearchSnapshots()
}
