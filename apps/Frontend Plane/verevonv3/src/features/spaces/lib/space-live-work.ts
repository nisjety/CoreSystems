import { createRoot, createSignal } from 'solid-js'

import type { SpaceThread } from '@/shared/api/spaces-client'
import { LIVE_RUN_STATUSES } from './space-thread-presentation'

/**
 * What is live in each room right now, shared across the shell.
 *
 * # Why a module-level store and not a second fetch
 *
 * `SpacePage` already polls the room's thread projection every six seconds
 * while the tab is visible (item 1). The sidebar rendered the same projection
 * once, on mount, and never again — so its "Jobber" label was true at the
 * moment the panel opened and stale for the rest of the session. Fetching it a
 * second time on a second timer would put two answers to one question on the
 * same screen. Publishing the page's already-fetched projection here gives the
 * sidebar and the composer the same truth at the same moment for free.
 *
 * # Two sources, deliberately kept apart
 *
 * `projection` is what the server says about every thread in the room, and it
 * arrives on the poll. `ownStream` is what THIS browser knows about the turn it
 * is streaming right now, and it is immediate. Merging them would let the local
 * signal masquerade as a server fact; keeping them apart lets the sender see
 * their own indicator instantly while every other member sees it on the next
 * poll — and lets a consumer say which kind of knowledge it is acting on.
 *
 * # Absence is not idleness
 *
 * The store only holds rooms whose page has published. A room with no entry
 * has not been observed, and `liveThreadsIn` returns an empty list for it —
 * callers must treat that as "unknown", never as "nothing is running". The
 * sidebar shows no dot for an unobserved room rather than a calm one.
 */
export interface OwnStream {
  readonly spaceRef: string
  /** The thread being continued, when the turn replies into one. */
  readonly threadId?: string
}

interface LiveWorkState {
  readonly projections: ReadonlyMap<string, readonly SpaceThread[]>
  readonly ownStream: OwnStream | undefined
  /**
   * Posts new since the reader arrived, per room (item 4b). Published by the
   * page from its arrival-time read marker so the sidebar can badge the same
   * rows the timeline does, from the same derivation, without a second read.
   */
  readonly unread: ReadonlyMap<string, ReadonlySet<string>>
}

const store = createRoot(() => {
  const [state, setState] = createSignal<LiveWorkState>({
    projections: new Map(),
    ownStream: undefined,
    unread: new Map(),
  })
  return { state, setState }
})

/** Publish a room's thread projection. Called by whoever fetched it. */
export function publishSpaceThreads(spaceRef: string, threads: readonly SpaceThread[]): void {
  const ref = spaceRef.trim()
  if (!ref) return
  store.setState((current) => {
    const projections = new Map(current.projections)
    projections.set(ref, threads)
    return { ...current, projections }
  })
}

/** The page left the room; its projection is no longer being refreshed. */
export function retractSpaceThreads(spaceRef: string): void {
  const ref = spaceRef.trim()
  store.setState((current) => {
    if (!current.projections.has(ref) && !current.unread.has(ref)) return current
    const projections = new Map(current.projections)
    projections.delete(ref)
    const unread = new Map(current.unread)
    unread.delete(ref)
    return { ...current, projections, unread }
  })
}

/** Publish which posts are new since the reader arrived in a room. */
export function publishSpaceUnread(spaceRef: string, threadIds: ReadonlySet<string>): void {
  const ref = spaceRef.trim()
  if (!ref) return
  store.setState((current) => {
    const unread = new Map(current.unread)
    unread.set(ref, new Set(threadIds))
    return { ...current, unread }
  })
}

/** Posts new since the reader arrived. Empty for an unobserved room. */
export function unreadThreadIdsIn(spaceRef: string): ReadonlySet<string> {
  return store.state().unread.get(spaceRef.trim()) ?? new Set()
}

/** This browser started streaming a turn. */
export function beginOwnStream(stream: OwnStream): void {
  store.setState((current) => ({ ...current, ownStream: stream }))
}

/** This browser's turn ended — completed, failed, or stopped. */
export function endOwnStream(): void {
  store.setState((current) =>
    current.ownStream ? { ...current, ownStream: undefined } : current,
  )
}

/**
 * Threads the server reports as live in a room. Empty when the room has not
 * been observed — see the module note on absence.
 */
export function liveThreadsIn(spaceRef: string): readonly SpaceThread[] {
  const threads = store.state().projections.get(spaceRef.trim()) ?? []
  return threads.filter((thread) => LIVE_RUN_STATUSES.has(thread.latest_run_status ?? ''))
}

/** Whether this room has been published at all. */
export function isSpaceObserved(spaceRef: string): boolean {
  return store.state().projections.has(spaceRef.trim())
}

/** The turn this browser is streaming, if any. */
export function ownStream(): OwnStream | undefined {
  return store.state().ownStream
}

/**
 * Whether anything is live in the room, from either source. A consumer that
 * needs to know WHICH source should read `liveThreadsIn` and `ownStream`
 * directly rather than this convenience.
 */
export function isSpaceWorking(spaceRef: string): boolean {
  const ref = spaceRef.trim()
  return liveThreadsIn(ref).length > 0 || ownStream()?.spaceRef === ref
}

/** Test seam: forget everything. */
export function resetLiveWorkForTests(): void {
  store.setState({ projections: new Map(), ownStream: undefined, unread: new Map() })
}
