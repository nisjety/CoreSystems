import type { SpaceReadMarker, SpaceThread } from '@/shared/api/spaces-client'

/**
 * Which posts are new since the reader last had the room open (item 4b).
 *
 * A deliberate mirror of Application Plane's `unreadThreadIds` in
 * `convex/spaceReadMarkers.ts`, kept in the browser so the room can badge
 * posts the moment the listing arrives rather than after a second round trip.
 * The two must agree; if this file and that one ever differ, the Convex one
 * is the owner and this one is wrong.
 *
 * - No marker (never caught up) badges nothing: there is no last visit to be
 *   new since, and badging every post on a first visit teaches people to
 *   ignore the badge.
 * - The reader's own posts are never new to them.
 * - A post with no readable timestamp is left alone rather than guessed at.
 *
 * `asOf` is the marker snapshotted when the visit STARTED. The page advances
 * the durable marker while the room stays open, but the badges must keep
 * pointing at what was new when the reader arrived — otherwise they would
 * vanish on the first poll, before anyone had read anything.
 */
export function unreadThreadIds(
  threads: readonly SpaceThread[],
  asOf: SpaceReadMarker | undefined,
  viewerSubjectId: string | undefined,
): ReadonlySet<string> {
  const lastReadAt = asOf?.last_read_at
  if (lastReadAt === null || lastReadAt === undefined || !Number.isFinite(lastReadAt)) {
    return new Set()
  }
  const viewer = (viewerSubjectId ?? '').trim()
  const out = new Set<string>()
  for (const thread of threads) {
    const id = thread.thread_id?.trim()
    if (!id) continue
    if (viewer && (thread.owner_subject_id ?? '').trim() === viewer) continue
    const at = latestActivityMillis(thread)
    if (at === undefined) continue
    if (at > lastReadAt) out.add(id)
  }
  return out
}

function latestActivityMillis(thread: SpaceThread): number | undefined {
  const candidates = [thread.updated_at, thread.latest_run_updated_at]
    .map((value) => (typeof value === 'string' && value.trim() ? Date.parse(value) : Number.NaN))
    .filter((value) => Number.isFinite(value))
  if (candidates.length === 0) return undefined
  return Math.max(...candidates)
}
