import { createSignal } from 'solid-js'
import type { Signal } from 'solid-js'

import { getSpaceTimeline, loadSpaceTranscript } from '@/shared/api/spaces-client'
import type {
  SpaceThread,
  SpaceThreads,
  SpaceTimeline,
  SpaceTranscriptDenial,
} from '@/shared/api/spaces-client'
import { normalizeTranscriptTurns } from '../components/sidepane/SpaceTurnList'
import type { TranscriptTurn } from '../components/sidepane/SpaceTurnList'
import { AWAITING_APPROVAL_RUN_STATUS, LIVE_RUN_STATUSES } from './space-thread-presentation'

/**
 * The windowed room timeline's client store (WS15-S8, `features.timeline_windows`).
 *
 * It holds the posts the reader has loaded as a set of keyset pages, so the room never holds more than
 * {@link TIMELINE_MAX_POSTS} posts and never needs an offset: "Load earlier" continues from the oldest
 * loaded page's cursor, `refresh()` continues from the newest page's cursor. Transcripts are lazy: a
 * post asks for its tail ({@link TAIL_TURNS} turns plus the first) only when it is near the viewport,
 * four requests at a time.
 *
 * Authority: nothing here is a grant. Every page and every transcript is read through the gateway,
 * which re-checks the Space lifecycle, the Control membership and mints its own read decision for each
 * request; a cursor only says where to continue. The store keeps its data in memory only and writes
 * nothing to browser storage, so a Zero Data Retention room leaves nothing behind.
 *
 * It does not depend on the DOM: the component tells it which posts are visible.
 */

/** Posts per page. The gateway restates the bounds (1 to 100). */
export const TIMELINE_PAGE_LIMIT = 50
/** The most posts held at once; beyond it the side farthest from the reader is dropped, a page at a time. */
export const TIMELINE_MAX_POSTS = 400
/** Transcript reads in flight at once. */
export const TRANSCRIPT_CONCURRENCY = 4
/** Newest turns asked of a post's transcript (matches `TAIL_WINDOW_TURNS`). */
export const TAIL_TURNS = 12
/** Most `after` pages one refresh follows before it stops and offers "Load newer". */
const MAX_REFRESH_PAGES = 4
/** Most thread ids one refresh names (the gateway's bound). */
const MAX_REFRESH_IDS = 30

export type TimelineTranscriptState = 'ok' | 'restricted' | 'missing' | 'error'

export type TimelineTranscript = {
  readonly state: TimelineTranscriptState
  /** The post's first turn (when turns were left out) followed by the tail, or the whole transcript. */
  readonly turns: readonly TranscriptTurn[]
  readonly denial?: SpaceTranscriptDenial
  /** Turns between the first turn and the loaded tail that are not loaded. Zero when everything is. */
  readonly omittedBefore: number
  /** The revision of the post this was read for, to tell when the post has moved on. */
  readonly rev: string
  /** A (re)read is in flight. The previous turns stay on screen meanwhile. */
  readonly loading: boolean
  readonly earlierLoading: boolean
}

type Page = {
  readonly ids: Set<string>
  /** Continues in the older direction from this page's oldest post; null when nothing is older. */
  olderCursor: string | null
  /** Where an `after` read continues from this page's newest post. */
  newestCursor: string | null
}

/** The revision a transcript was read for. A change means the post has new turns or a new run state. */
export function threadRevision(thread: SpaceThread): string {
  return `${thread.updated_at ?? thread.latest_run_updated_at ?? ''}|${thread.last_message_seq ?? ''}`
}

function orderKey(thread: SpaceThread): [number, string] {
  return [thread.last_message_seq ?? 0, thread.updated_at ?? thread.latest_run_updated_at ?? '']
}

/** Oldest first. Falls back to the time when a side has no sequence (a legacy listing). */
function compareThreads(a: SpaceThread, b: SpaceThread): number {
  const [aSeq, aAt] = orderKey(a)
  const [bSeq, bAt] = orderKey(b)
  if (aSeq > 0 && bSeq > 0 && aSeq !== bSeq) return aSeq - bSeq
  return aAt.localeCompare(bAt)
}

export type SpaceTimelineStore = ReturnType<typeof createSpaceTimelineStore>

export function createSpaceTimelineStore(spaceRef: string) {
  const items = new Map<string, SpaceThread>()
  let pages: Page[] = []
  let head: SpaceThreads | undefined
  let pinned: readonly SpaceThread[] = []
  let total: number | null = null
  let legacy = false
  let newerHidden = false
  let disposed = false
  let loadingEarlierNow = false
  let loadingLaterNow = false
  let refreshing: Promise<void> | undefined
  let firstLoaded = false

  const [version, setVersion] = createSignal(0)
  const bump = () => setVersion((current) => current + 1)
  const [earlierVersion, setEarlierVersion] = createSignal(0)
  const [busy, setBusy] = createSignal<'idle' | 'earlier' | 'later'>('idle')

  // ── pages ──────────────────────────────────────────────────────────────────────────────────────
  function newestPage(): Page | undefined {
    return pages[pages.length - 1]
  }
  function oldestPage(): Page | undefined {
    return pages[0]
  }
  function assign(thread: SpaceThread, page: Page): void {
    for (const other of pages) if (other !== page) other.ids.delete(thread.thread_id)
    items.set(thread.thread_id, thread)
    page.ids.add(thread.thread_id)
  }
  function forget(id: string): void {
    items.delete(id)
    transcripts.delete(id)
    mirror.delete(id)
    visible.delete(id)
  }
  function replaceAll(threads: readonly SpaceThread[]): void {
    const keep = new Set(threads.map((thread) => thread.thread_id))
    for (const id of [...items.keys()]) if (!keep.has(id)) forget(id)
    const page: Page = { ids: new Set(), olderCursor: null, newestCursor: null }
    for (const thread of threads) {
      items.set(thread.thread_id, thread)
      page.ids.add(thread.thread_id)
    }
    pages = [page]
  }
  /** Drops whole pages from the far side until the cap holds. Keeps at least one page. */
  function trim(drop: 'newest' | 'oldest'): void {
    let trimmed = false
    while (items.size > TIMELINE_MAX_POSTS && pages.length > 1) {
      if (drop === 'newest') {
        const gone = pages.pop() as Page
        for (const id of gone.ids) forget(id)
        newerHidden = true
      } else {
        // The page that becomes the oldest must be able to continue older; a page that was read in
        // the newer direction carries no such cursor, so stop trimming rather than lose the way back.
        if (pages[1]?.olderCursor == null) break
        const gone = pages.shift() as Page
        for (const id of gone.ids) forget(id)
      }
      trimmed = true
    }
    if (trimmed) setEarlierVersion((current) => current + 1)
  }

  // ── reads of the state ─────────────────────────────────────────────────────────────────────────
  /** The loaded posts, oldest first. */
  function threads(): readonly SpaceThread[] {
    version()
    return [...items.values()].sort(compareThreads)
  }
  /** Pinned posts that are older than the loaded window, for the shelf above the list. */
  function pinnedShelf(): readonly SpaceThread[] {
    version()
    return pinned.filter((thread) => !items.has(thread.thread_id))
  }
  function hasEarlier(): boolean {
    version()
    return !legacy && oldestPage()?.olderCursor != null
  }
  function hasNewer(): boolean {
    version()
    return !legacy && newerHidden
  }
  function totalPosts(): number | null {
    version()
    return legacy ? items.size : total
  }
  /** Number of posts older than the loaded window, when the server said how many exist. */
  function unloadedBefore(): number {
    version()
    if (total === null || legacy) return 0
    return Math.max(0, total - items.size)
  }
  function windowed(): boolean {
    version()
    return !legacy
  }
  /** Bumps whenever a page is added at the top or posts are trimmed: the reader's baseline moved. */
  function windowVersion(): number {
    return earlierVersion()
  }
  function loadingMore(): 'idle' | 'earlier' | 'later' {
    return busy()
  }
  // One object per state, so a reader that asks twice between changes (the page's resource and its
  // effect) gets the same reference and no second update.
  let projected: { readonly at: number; readonly value: SpaceThreads } | undefined
  function projection(): SpaceThreads | undefined {
    const at = version()
    if (!head) return undefined
    if (projected?.at !== at) projected = { at, value: { ...head, threads: threads() } }
    return projected.value
  }

  // ── posts ──────────────────────────────────────────────────────────────────────────────────────
  function takeHead(timeline: SpaceTimeline): void {
    if (timeline.head) head = timeline.head
    pinned = timeline.pinned
    if (timeline.page.totalPosts !== null) total = timeline.page.totalPosts
  }

  /** The first page, merged. Throws what the request threw so the caller can read a 404 as "off". */
  async function loadFirst(): Promise<void> {
    const timeline = await getSpaceTimeline(spaceRef, { limit: TIMELINE_PAGE_LIMIT })
    if (disposed) return
    takeHead(timeline)
    if (!timeline.page.windowed) {
      legacy = true
      newerHidden = false
      replaceAll(timeline.threads)
      total = timeline.threads.length
    } else if (!firstLoaded || pages.length === 0) {
      legacy = false
      const page: Page = { ids: new Set(), olderCursor: timeline.page.nextBefore, newestCursor: timeline.page.newest }
      pages = [page]
      for (const thread of timeline.threads) assign(thread, page)
    } else {
      // A reload of the head (the first page's extras changed): posts already loaded are updated in
      // place; posts that are new arrive through `refresh`, which continues without a gap.
      for (const thread of timeline.threads) if (items.has(thread.thread_id)) items.set(thread.thread_id, thread)
    }
    firstLoaded = true
    bump()
  }

  /** Posts whose state can change without their tail moving: running, waiting, or on screen. */
  function refreshIds(): string[] {
    const ids = new Set<string>()
    for (const thread of items.values()) {
      const status = thread.latest_run_status ?? ''
      if (LIVE_RUN_STATUSES.has(status) || status === AWAITING_APPROVAL_RUN_STATUS) ids.add(thread.thread_id)
    }
    for (const id of visible) if (items.has(id)) ids.add(id)
    return [...ids].slice(0, MAX_REFRESH_IDS)
  }

  function applyRefreshed(refreshed: readonly SpaceThread[]): void {
    for (const thread of refreshed) if (items.has(thread.thread_id)) items.set(thread.thread_id, thread)
  }

  async function runRefresh(): Promise<void> {
    if (!firstLoaded) {
      await loadFirst()
      return
    }
    if (legacy) {
      // The legacy listing is the whole room every time.
      const timeline = await getSpaceTimeline(spaceRef, { limit: TIMELINE_PAGE_LIMIT })
      if (disposed) return
      takeHead(timeline)
      if (timeline.page.windowed) {
        // The server finished its backfill: start over as a windowed room.
        legacy = false
        newerHidden = false
        const page: Page = { ids: new Set(), olderCursor: timeline.page.nextBefore, newestCursor: timeline.page.newest }
        replaceAll([])
        pages = [page]
        for (const thread of timeline.threads) assign(thread, page)
      } else {
        replaceAll(timeline.threads)
        total = timeline.threads.length
      }
      bump()
      return
    }
    for (let round = 0; round < MAX_REFRESH_PAGES; round += 1) {
      const page = newestPage()
      const cursor = page?.newestCursor ?? null
      const ids = refreshIds()
      // While newer pages are hidden an `after` read would leave a gap: only re-read what is loaded.
      const timeline = await getSpaceTimeline(spaceRef, {
        limit: TIMELINE_PAGE_LIMIT,
        ...(newerHidden || cursor === null ? {} : { after: cursor }),
        ...(ids.length > 0 ? { refresh: ids } : {}),
      })
      if (disposed) return
      if (!timeline.page.windowed) {
        legacy = true
        takeHead(timeline)
        replaceAll(timeline.threads)
        total = timeline.threads.length
        bump()
        return
      }
      applyRefreshed(timeline.refreshed)
      if (newerHidden) {
        // A re-read of loaded posts only; the first-page request (no cursor) also carries the head.
        applyRefreshed(timeline.threads)
        bump()
        return
      }
      if (cursor === null) {
        // No cursor yet: the room was empty, so this is its first page.
        takeHead(timeline)
        const fresh: Page = { ids: new Set(), olderCursor: timeline.page.nextBefore, newestCursor: timeline.page.newest }
        pages = [fresh]
        for (const thread of timeline.threads) assign(thread, fresh)
        bump()
        return
      }
      let added = 0
      for (const thread of timeline.threads) {
        if (!items.has(thread.thread_id)) added += 1
        assign(thread, page as Page)
      }
      if (total !== null) total += added
      if (timeline.page.newest) (page as Page).newestCursor = timeline.page.newest
      trim('oldest')
      bump()
      if (!timeline.page.hasMore) return
      if (round === MAX_REFRESH_PAGES - 1) {
        // Still more to read after the bound: stop and let the reader ask for it.
        newerHidden = true
        bump()
      }
    }
  }

  /** Brings the loaded window up to date: new posts after the newest cursor, plus a re-read of the posts that can change. */
  function refresh(): Promise<void> {
    if (refreshing) return refreshing
    const run = runRefresh().finally(() => {
      if (refreshing === run) refreshing = undefined
    })
    refreshing = run
    return run
  }

  /** Re-reads the first page's extras (pins, agent work, shared read) and then catches up. */
  async function reloadHead(): Promise<void> {
    if (!firstLoaded) return loadFirst()
    const timeline = await getSpaceTimeline(spaceRef, { limit: TIMELINE_PAGE_LIMIT })
    if (disposed) return
    takeHead(timeline)
    if (!timeline.page.windowed) {
      legacy = true
      replaceAll(timeline.threads)
      total = timeline.threads.length
    } else {
      for (const thread of timeline.threads) if (items.has(thread.thread_id)) items.set(thread.thread_id, thread)
    }
    bump()
    if (!legacy) await refresh()
  }

  async function loadEarlier(): Promise<void> {
    if (loadingEarlierNow || legacy) return
    const cursor = oldestPage()?.olderCursor ?? null
    if (cursor === null) return
    loadingEarlierNow = true
    setBusy('earlier')
    try {
      const timeline = await getSpaceTimeline(spaceRef, { limit: TIMELINE_PAGE_LIMIT, before: cursor })
      if (disposed || !timeline.page.windowed) return
      const page: Page = { ids: new Set(), olderCursor: timeline.page.nextBefore, newestCursor: timeline.page.newest }
      // A post that moved to the newer end while the reader paged back is already loaded, newer.
      for (const thread of timeline.threads) {
        if (items.has(thread.thread_id)) continue
        items.set(thread.thread_id, thread)
        page.ids.add(thread.thread_id)
      }
      pages = [page, ...pages]
      trim('newest')
      setEarlierVersion((current) => current + 1)
      bump()
    } finally {
      loadingEarlierNow = false
      setBusy('idle')
    }
  }

  /** The newer posts hidden by a trim (or by a refresh that was cut short), read forward from the newest page. */
  async function loadLater(): Promise<void> {
    if (loadingLaterNow || legacy || !newerHidden) return
    const cursor = newestPage()?.newestCursor ?? null
    if (cursor === null) return
    loadingLaterNow = true
    setBusy('later')
    try {
      const timeline = await getSpaceTimeline(spaceRef, { limit: TIMELINE_PAGE_LIMIT, after: cursor })
      if (disposed || !timeline.page.windowed) return
      const page: Page = { ids: new Set(), olderCursor: null, newestCursor: timeline.page.newest ?? cursor }
      for (const thread of timeline.threads) assign(thread, page)
      pages = [...pages, page]
      newerHidden = timeline.page.hasMore
      trim('oldest')
      setEarlierVersion((current) => current + 1)
      bump()
    } finally {
      loadingLaterNow = false
      setBusy('idle')
    }
  }

  // ── visibility ─────────────────────────────────────────────────────────────────────────────────
  const visible = new Set<string>()
  function setVisible(id: string, isVisible: boolean): void {
    if (isVisible) visible.add(id)
    else visible.delete(id)
  }

  // ── transcripts ────────────────────────────────────────────────────────────────────────────────
  const transcripts = new Map<string, Signal<TimelineTranscript | undefined>>()
  // The same values in a plain map: the store's own logic reads these, so it never reads a signal from
  // inside an effect's untracked half. Only `transcript()` (the render path) reads the signal.
  const mirror = new Map<string, TimelineTranscript>()
  function slot(id: string): Signal<TimelineTranscript | undefined> {
    let entry = transcripts.get(id)
    if (!entry) {
      entry = createSignal<TimelineTranscript>()
      transcripts.set(id, entry)
    }
    return entry
  }
  function write(id: string, next: TimelineTranscript | undefined): void {
    if (next) mirror.set(id, next)
    else mirror.delete(id)
    const [, set] = slot(id)
    set(() => next)
  }
  /** What the post draws. `undefined` until the post has asked. Reactive. */
  function transcript(id: string): TimelineTranscript | undefined {
    return slot(id)[0]()
  }

  const queued = new Map<string, string>()
  const inFlight = new Map<string, string>()

  /**
   * Asks for a post's transcript tail unless the current one is for this revision. Called by a post
   * that is near the viewport; a changed revision re-reads (the old turns stay until the new arrive).
   */
  function ensureTranscript(thread: SpaceThread): void {
    if (disposed) return
    const id = thread.thread_id
    const rev = threadRevision(thread)
    const current = mirror.get(id)
    if (current && current.rev === rev && current.state !== 'error') return
    if (queued.get(id) === rev || inFlight.get(id) === rev) return
    queued.set(id, rev)
    pump()
  }
  function retryTranscript(id: string): void {
    const thread = items.get(id)
    if (!thread) return
    write(id, undefined)
    queued.delete(id)
    inFlight.delete(id)
    ensureTranscript(thread)
  }
  function pump(): void {
    while (inFlight.size < TRANSCRIPT_CONCURRENCY && queued.size > 0) {
      // Newest first: the reader starts at the bottom, so the posts nearest the viewport are the newest.
      let pick: string | undefined
      let pickThread: SpaceThread | undefined
      for (const id of queued.keys()) {
        const thread = items.get(id)
        if (!thread) {
          queued.delete(id)
          continue
        }
        if (!pickThread || compareThreads(thread, pickThread) > 0) {
          pick = id
          pickThread = thread
        }
      }
      if (!pick || !pickThread) return
      const rev = queued.get(pick) as string
      queued.delete(pick)
      inFlight.set(pick, rev)
      void readTail(pick, rev)
    }
  }
  async function readTail(id: string, rev: string): Promise<void> {
    const previous = mirror.get(id)
    if (previous) write(id, { ...previous, loading: true })
    else write(id, { state: 'ok', turns: [], omittedBefore: 0, rev: '', loading: true, earlierLoading: false })
    let next: TimelineTranscript
    try {
      const load = await loadSpaceTranscript(spaceRef, id, { tail: TAIL_TURNS, includeFirst: true })
      if (load.state === 'ok') {
        next = {
          state: 'ok',
          turns: normalizeTranscriptTurns(load.turns as readonly unknown[], Number.MAX_SAFE_INTEGER),
          omittedBefore: load.window?.omittedBefore ?? 0,
          rev,
          loading: false,
          earlierLoading: false,
        }
      } else if (load.state === 'error' && previous && previous.state === 'ok' && previous.turns.length > 0) {
        // A failed re-read keeps what the reader already has.
        next = { ...previous, loading: false }
      } else {
        next = {
          state: load.state,
          turns: [],
          ...(load.state === 'restricted' && load.denial ? { denial: load.denial } : {}),
          omittedBefore: 0,
          rev,
          loading: false,
          earlierLoading: false,
        }
      }
    } catch {
      next = { state: 'error', turns: [], omittedBefore: 0, rev, loading: false, earlierLoading: false }
    }
    inFlight.delete(id)
    if (!disposed && items.has(id)) write(id, next)
    pump()
  }

  /** Reads the next window of turns before the loaded tail and slots it in after the first turn. */
  async function loadEarlierTurns(id: string): Promise<void> {
    const current = mirror.get(id)
    if (!current || current.state !== 'ok' || current.omittedBefore <= 0 || current.earlierLoading) return
    const oldestTail = current.turns[1]?.messageId
    if (!oldestTail) return
    write(id, { ...current, earlierLoading: true })
    const load = await loadSpaceTranscript(spaceRef, id, { tail: TAIL_TURNS, before: oldestTail })
    const latest = mirror.get(id) ?? current
    if (disposed || load.state !== 'ok') {
      if (!disposed) write(id, { ...latest, earlierLoading: false })
      return
    }
    const fetched = normalizeTranscriptTurns(load.turns as readonly unknown[], Number.MAX_SAFE_INTEGER)
    const have = new Set(latest.turns.map((turn) => turn.messageId).filter((value): value is string => Boolean(value)))
    const fresh = fetched.filter((turn) => !turn.messageId || !have.has(turn.messageId))
    const reachedFirst = fetched.some((turn) => turn.messageId !== undefined && turn.messageId === latest.turns[0]?.messageId)
    const [first, ...rest] = latest.turns
    const remaining = reachedFirst || load.window?.hasMoreBefore !== true
      ? 0
      : Math.max(0, latest.omittedBefore - fresh.length)
    write(id, {
      ...latest,
      turns: first ? [first, ...fresh, ...rest] : [...fresh, ...rest],
      omittedBefore: remaining,
      earlierLoading: false,
    })
  }

  function dispose(): void {
    disposed = true
    queued.clear()
  }

  return {
    // posts
    threads,
    pinnedShelf,
    projection,
    hasEarlier,
    hasNewer,
    totalPosts,
    unloadedBefore,
    windowed,
    windowVersion,
    loadingMore,
    loadFirst,
    refresh,
    reloadHead,
    loadEarlier,
    loadLater,
    setVisible,
    // transcripts
    transcript,
    ensureTranscript,
    retryTranscript,
    loadEarlierTurns,
    dispose,
  }
}
