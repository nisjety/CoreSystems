// WS15-S8: the windowed room timeline. `GET /api/v1/spaces/{space_ref}/timeline` (R1, WS15-S7) answers
// one page of the room's posts, oldest first within the page, with an opaque keyset cursor. The first
// page also carries what the legacy listing carried (membership, pins, agent work, read marker); later
// pages and refreshes carry posts only. With the gateway flag off the route is a 404
// `space_timeline_disabled`, which callers read as "use the legacy listing". When Session Core is still
// building its summary the gateway answers the legacy listing shaped as one complete page
// (`page.windowed === false`).
import { requestJson } from '../http'
import { spaceThreadsFromBody } from './core'
import type { SpaceThread, SpaceThreads, SpaceThreadsBody } from './core'

export type SpaceTimelinePage = {
  /** False when the gateway served the legacy listing: the threads are the whole room. */
  readonly windowed: boolean
  readonly hasMore: boolean
  /** Continues in the older direction ("Load earlier"). */
  readonly nextBefore: string | null
  /** Continues in the newer direction when an `after` read was cut short. */
  readonly nextAfter: string | null
  /** The cursor of the newest post in this answer; `null` when the answer held no posts. */
  readonly newest: string | null
  /** First page only. */
  readonly totalPosts: number | null
}

/**
 * A room-level system row (WS15-S11/S12): one fact from Control's membership ledger or an Application
 * notice (agent bound, paused, resumed, revoked; room renamed; topic changed). Ids and closed
 * words only; the reader resolves names from the roster. Same shape as the `system` timeline entry
 * of the Space feature's entry registry.
 */
export type SpaceSystemEntry = {
  readonly kind: 'system'
  readonly id: string
  /** ISO-8601 time the ledger recorded the change. */
  readonly at: string
  /** The ledger's closed action word, e.g. `member_added`. */
  readonly code: string
  readonly actorId?: string
  readonly subjectId?: string
  readonly fromRole?: string
  readonly toRole?: string
}

export type SpaceTimeline = {
  readonly page: SpaceTimelinePage
  /** The room's system rows for the interval this answer covers, oldest first. Empty when the gateway has the feature off. */
  readonly systemEntries: readonly SpaceSystemEntry[]
  /** The gateway cut the system rows to its per-page bound; the interval held more. */
  readonly systemTruncated: boolean
  /** Oldest first within the page (the gateway reverses Session Core's newest-first order). */
  readonly threads: readonly SpaceThread[]
  /** First page only: the room's pinned posts, which may be older than the window. */
  readonly pinned: readonly SpaceThread[]
  /** The posts asked for by `refresh`, as they are now. */
  readonly refreshed: readonly SpaceThread[]
  /** The legacy listing's extras. Present on a first page and on a legacy fallback, absent on later pages. */
  readonly head?: SpaceThreads
}

export type SpaceTimelineRequest = {
  readonly limit?: number
  readonly before?: string
  readonly after?: string
  /** Thread ids to re-read, at most 30. */
  readonly refresh?: readonly string[]
}

/** Most thread ids one refresh may name (the gateway's bound). */
export const SPACE_TIMELINE_MAX_REFRESH = 30

function threadsOf(value: unknown): readonly SpaceThread[] {
  if (!Array.isArray(value)) return []
  return value.filter(
    (item): item is SpaceThread =>
      Boolean(item) && typeof item === 'object' && typeof (item as { thread_id?: unknown }).thread_id === 'string',
  )
}

function systemEntriesOf(value: unknown): readonly SpaceSystemEntry[] {
  if (!Array.isArray(value)) return []
  const out: SpaceSystemEntry[] = []
  for (const item of value) {
    if (!item || typeof item !== 'object') continue
    const record = item as Record<string, unknown>
    const text = (key: string): string | undefined => {
      const field = record[key]
      return typeof field === 'string' && field.trim() ? field.trim() : undefined
    }
    const id = text('id')
    const code = text('action')
    const atRaw = text('created_at')
    const atMs = atRaw ? Date.parse(atRaw) : Number.NaN
    // A row with no id, no word or no readable time cannot be placed or de-duplicated: drop it.
    if (!id || !code || !Number.isFinite(atMs) || (record.kind !== 'membership' && record.kind !== 'notice')) continue
    out.push({
      kind: 'system',
      id,
      at: new Date(atMs).toISOString(),
      code,
      ...(text('actor_id') ? { actorId: text('actor_id') } : {}),
      ...(text('subject_id') ? { subjectId: text('subject_id') } : {}),
      ...(text('from_role') ? { fromRole: text('from_role') } : {}),
      ...(text('to_role') ? { toRole: text('to_role') } : {}),
    })
  }
  // The gateway sends newest first; the room reads oldest first.
  return out.sort((a, b) => a.at.localeCompare(b.at) || a.id.localeCompare(b.id))
}

function cursorOf(value: unknown): string | null {
  return typeof value === 'string' && value.trim() ? value.trim() : null
}

export async function getSpaceTimeline(spaceRef: string, request: SpaceTimelineRequest = {}): Promise<SpaceTimeline> {
  const params = new URLSearchParams()
  if (request.limit !== undefined) params.set('limit', String(request.limit))
  if (request.before) params.set('before', request.before)
  if (request.after) params.set('after', request.after)
  const refresh = (request.refresh ?? []).slice(0, SPACE_TIMELINE_MAX_REFRESH)
  if (refresh.length > 0) params.set('refresh', refresh.join(','))
  const query = params.toString()
  const response = await requestJson<Record<string, unknown>>(
    `/api/v1/spaces/${encodeURIComponent(spaceRef)}/timeline${query ? `?${query}` : ''}`,
  )
  const rawPage = (response.page && typeof response.page === 'object' ? response.page : {}) as Record<string, unknown>
  const windowed = rawPage.windowed === true
  const total = rawPage.total_posts
  const page: SpaceTimelinePage = {
    windowed,
    hasMore: windowed && rawPage.has_more === true,
    nextBefore: cursorOf(rawPage.next_before),
    nextAfter: cursorOf(rawPage.next_after),
    newest: cursorOf(rawPage.newest),
    totalPosts: typeof total === 'number' && Number.isFinite(total) && total >= 0 ? Math.floor(total) : null,
  }
  const threads = threadsOf(response.threads)
  const hasHead = Boolean(response.membership) && typeof response.membership === 'object'
  return {
    page,
    systemEntries: systemEntriesOf(response.system_entries),
    systemTruncated: response.system_truncated === true,
    threads,
    pinned:threadsOf(response.pinned),
    refreshed: threadsOf(response.refreshed),
    ...(hasHead ? { head: spaceThreadsFromBody({ ...(response as unknown as SpaceThreadsBody), threads }) } : {}),
  }
}
