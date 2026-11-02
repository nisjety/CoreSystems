/**
 * State and rules for in-room message search (WS13-S5).
 *
 * The controller is imperative on purpose. A keystroke schedules a debounced request, a newer
 * request aborts the older one, and a late answer for an older request is dropped. Solid 2 applies
 * signal writes at flush, so the logic here reads plain variables and mirrors them into signals only
 * for rendering; it never reads a signal right after writing it.
 *
 * Privacy: nothing is cached and nothing is written to storage while searching. Recent queries live
 * in memory and, only while `isLocalRetentionAllowed()` is true, under one localStorage key that a
 * retention denial removes. A search never changes what the viewer may read: every request goes to
 * the gateway, which asks Control, and the answer says how much of the room it covered.
 */

import { createSignal } from 'solid-js'

import { isLocalRetentionAllowed, subscribeLocalRetention } from '@/features/chat/lib/chat-retention'
import {
  searchSpaceMessages,
  type SpaceSearchCoverage,
  type SpaceSearchHit,
  type SpaceSearchPage,
  type SpaceSearchRequest,
  type SpaceSearchScope,
} from '@/shared/api/spaces-client'
import { ApiError } from '@/shared/api/http'
import { spaceRoomFeatureWithDevOverride } from './space-room-features'

export const SEARCH_MIN_CHARS = 2
export const SEARCH_MAX_CHARS = 200
export const SEARCH_DEBOUNCE_MS = 250
export const SEARCH_PAGE_SIZE = 20
export const RECENT_SEARCHES_KEY = 'verevon.space.search.recent.v1'
const RECENT_SEARCHES_MAX = 8

/**
 * Whether in-room search shows (`features.message_search` from the room context, which is the
 * gateway's API flag and UI flag together). A dev build may override it with
 * `VITE_SPACE_MESSAGE_SEARCH_UI=true|false`. A rollout control, never an authorization input.
 */
export function spaceMessageSearchEnabled(): boolean {
  return spaceRoomFeatureWithDevOverride(
    'message_search',
    (import.meta.env as Record<string, string | undefined>).VITE_SPACE_MESSAGE_SEARCH_UI,
  )
}

export type SearchPhase = 'idle' | 'searching' | 'results' | 'empty' | 'error'

/** The distinct ways a search fails, each with its own sentence. */
export type SearchFailure = 'busy' | 'timeout' | 'unavailable' | 'disabled' | 'forbidden' | 'invalid' | 'other'

/** Names the failure for the UI; the copy for it lives in the component and in errors.ts. */
export function searchFailureKind(error: unknown): SearchFailure {
  if (!(error instanceof ApiError)) return 'other'
  switch (error.code) {
    case 'search_busy':
      return 'busy'
    case 'search_timeout':
      return 'timeout'
    case 'space_search_disabled':
      return 'disabled'
    case 'active_membership_required':
    case 'space_read_not_authorized':
    case 'space_not_found':
      return 'forbidden'
    case 'invalid_query':
    case 'invalid_filter':
    case 'invalid_cursor':
      return 'invalid'
    case 'search_unavailable':
    case 'space_lifecycle_unavailable':
    case 'space_decision_unavailable':
    case 'delegated_auth_unavailable':
    case 'invalid_model_gateway_response':
      return 'unavailable'
    default:
      return error.status >= 500 ? 'unavailable' : 'other'
  }
}

/** Failures worth a retry button: the request was fine, the service was not. */
export function searchFailureRetryable(kind: SearchFailure): boolean {
  return kind === 'busy' || kind === 'timeout' || kind === 'unavailable' || kind === 'other'
}

/** The `excluded` codes the UI explains. An unknown code is ignored rather than guessed at. */
export type SearchExclusionNote = 'no_searchable_terms' | 'zdr_waived_threads'

export function searchExclusionNotes(excluded: readonly string[]): readonly SearchExclusionNote[] {
  const notes: SearchExclusionNote[] = []
  if (excluded.includes('no_searchable_terms')) notes.push('no_searchable_terms')
  if (excluded.includes('zdr_waived_threads')) notes.push('zdr_waived_threads')
  return notes
}

// ── Recent queries ──────────────────────────────────────────────────────────────────────────────

let recentQueries: string[] = []
let recentHydrated = false

function readStoredRecents(): string[] {
  try {
    const raw = window.localStorage.getItem(RECENT_SEARCHES_KEY)
    const parsed: unknown = raw ? JSON.parse(raw) : []
    if (!Array.isArray(parsed)) return []
    return parsed
      .filter((item): item is string => typeof item === 'string')
      .map((item) => item.trim())
      .filter((item) => item.length >= SEARCH_MIN_CHARS && item.length <= SEARCH_MAX_CHARS)
      .slice(0, RECENT_SEARCHES_MAX)
  } catch {
    return []
  }
}

function writeStoredRecents(): void {
  try {
    if (isLocalRetentionAllowed()) window.localStorage.setItem(RECENT_SEARCHES_KEY, JSON.stringify(recentQueries))
  } catch {
    // Storage can be blocked or full; the in-memory list still works.
  }
}

function removeStoredRecents(): void {
  try {
    window.localStorage.removeItem(RECENT_SEARCHES_KEY)
  } catch {
    // Nothing to remove if storage is unreachable.
  }
}

/** Recent queries, newest first. Reads the stored copy once, and only while local retention is allowed. */
export function readRecentSearches(): readonly string[] {
  if (!recentHydrated) {
    recentHydrated = true
    if (isLocalRetentionAllowed()) {
      const stored = readStoredRecents()
      recentQueries = [...recentQueries, ...stored.filter((item) => !recentQueries.includes(item))].slice(
        0,
        RECENT_SEARCHES_MAX,
      )
    } else {
      removeStoredRecents()
    }
  }
  return recentQueries
}

/** Remembers one finished query (a submitted search or an opened hit), not each keystroke. */
export function rememberSearch(query: string): void {
  const text = query.trim()
  if (text.length < SEARCH_MIN_CHARS || text.length > SEARCH_MAX_CHARS) return
  readRecentSearches()
  recentQueries = [text, ...recentQueries.filter((item) => item !== text)].slice(0, RECENT_SEARCHES_MAX)
  writeStoredRecents()
}

/** Forgets every recent query, in memory and in storage. */
export function forgetRecentSearches(): void {
  recentQueries = []
  removeStoredRecents()
}

/** While mounted, a retention denial removes the stored copy at once. Returns the disposer. */
export function watchRecentSearchRetention(): () => void {
  return subscribeLocalRetention((allowed) => {
    if (!allowed) removeStoredRecents()
  })
}

export function __resetRecentSearchesForTests(): void {
  recentQueries = []
  recentHydrated = false
}

// ── The controller ──────────────────────────────────────────────────────────────────────────────

export interface SpaceSearchControllerOptions {
  readonly spaceRef: () => string
  /** The thread open in the side pane, when there is one. Scope `thread` needs it. */
  readonly threadId: () => string | null | undefined
  /** Injected for tests; defaults to the gateway call. */
  readonly search?: (spaceRef: string, request: SpaceSearchRequest, signal: AbortSignal) => Promise<SpaceSearchPage>
}

export interface SpaceSearchController {
  readonly query: () => string
  /**
   * The scope the chips show: `thread` reads as `room` once no thread is open, and `person` stays
   * chosen while no person is picked yet (requests use `room` until one is).
   */
  readonly scope: () => SpaceSearchScope
  readonly personId: () => string | null
  readonly phase: () => SearchPhase
  readonly hits: () => readonly SpaceSearchHit[]
  readonly coverage: () => SpaceSearchCoverage | null
  readonly truncated: () => boolean
  readonly excluded: () => readonly string[]
  readonly hasMore: () => boolean
  readonly loadingMore: () => boolean
  readonly failure: () => SearchFailure | null
  readonly activeIndex: () => number
  /** The text the current results answer, for the "no results for ..." line. */
  readonly answeredQuery: () => string
  setQuery(value: string): void
  /** Enter: search now, as a finished word (no prefix match), and put the first hit in focus. */
  submit(): void
  setScope(scope: SpaceSearchScope): void
  setPerson(subjectId: string | null): void
  setActiveIndex(index: number): void
  loadMore(): void
  retry(): void
  /** Aborts anything in flight and forgets the query and every result. */
  reset(): void
  dispose(): void
}

export function createSpaceSearchController(options: SpaceSearchControllerOptions): SpaceSearchController {
  const run = options.search ?? searchSpaceMessages

  // Plain state the logic reads.
  let queryValue = ''
  let scopeValue: SpaceSearchScope = 'room'
  let personValue: string | null = null
  let prefixMode = true
  let hitsValue: readonly SpaceSearchHit[] = []
  let cursorValue: string | null = null
  let lastRequest: SpaceSearchRequest | null = null
  let sequence = 0
  let timer: ReturnType<typeof setTimeout> | undefined
  let inFlight: AbortController | undefined

  // Mirrors for rendering.
  const [query, setQueryView] = createSignal('')
  const [scopeView, setScopeView] = createSignal<SpaceSearchScope>('room')
  const [person, setPersonView] = createSignal<string | null>(null)
  const [phase, setPhase] = createSignal<SearchPhase>('idle')
  const [hits, setHits] = createSignal<readonly SpaceSearchHit[]>([])
  const [coverage, setCoverage] = createSignal<SpaceSearchCoverage | null>(null)
  const [truncated, setTruncated] = createSignal(false)
  const [excluded, setExcluded] = createSignal<readonly string[]>([])
  const [hasMore, setHasMore] = createSignal(false)
  const [loadingMore, setLoadingMore] = createSignal(false)
  const [failure, setFailure] = createSignal<SearchFailure | null>(null)
  const [activeIndex, setActive] = createSignal(-1)
  const [answeredQuery, setAnsweredQuery] = createSignal('')

  const effectiveScope = (): SpaceSearchScope => {
    if (scopeValue === 'thread' && options.threadId()) return 'thread'
    if (scopeValue === 'person' && personValue) return 'person'
    return 'room'
  }

  const cancelPending = () => {
    if (timer !== undefined) clearTimeout(timer)
    timer = undefined
    inFlight?.abort()
    inFlight = undefined
    sequence += 1
  }

  const clearResults = () => {
    hitsValue = []
    cursorValue = null
    lastRequest = null
    setHits([])
    setCoverage(null)
    setTruncated(false)
    setExcluded([])
    setHasMore(false)
    setLoadingMore(false)
    setFailure(null)
    setActive(-1)
    setAnsweredQuery('')
  }

  const execute = (cursor: string | null) => {
    const text = queryValue.trim().slice(0, SEARCH_MAX_CHARS)
    if (text.length < SEARCH_MIN_CHARS) {
      cancelPending()
      clearResults()
      setPhase('idle')
      return
    }
    let request: SpaceSearchRequest
    if (cursor && lastRequest) {
      request = { ...lastRequest, cursor }
    } else {
      const scope = effectiveScope()
      request = {
        query: text,
        prefix: prefixMode,
        scope,
        threadId: scope === 'thread' ? (options.threadId() ?? undefined) : undefined,
        authorSubjectId: scope === 'person' ? (personValue ?? undefined) : undefined,
        limit: SEARCH_PAGE_SIZE,
      }
      lastRequest = request
    }
    inFlight?.abort()
    const controller = new AbortController()
    inFlight = controller
    const mine = ++sequence
    if (cursor) setLoadingMore(true)
    else {
      setPhase('searching')
      setFailure(null)
    }
    void run(options.spaceRef(), request, controller.signal).then(
      (page) => {
        if (mine !== sequence) return
        inFlight = undefined
        const seen = new Set(cursor ? hitsValue.map((hit) => hit.messageId) : [])
        const fresh = page.hits.filter((hit) => !seen.has(hit.messageId))
        hitsValue = cursor ? [...hitsValue, ...fresh] : fresh
        cursorValue = page.nextCursor
        setHits(hitsValue)
        setCoverage(page.coverage)
        setTruncated(page.truncated)
        setExcluded(page.excluded)
        setHasMore(page.nextCursor !== null)
        setLoadingMore(false)
        setFailure(null)
        setAnsweredQuery(request.query)
        setPhase(hitsValue.length > 0 ? 'results' : 'empty')
        if (!cursor) setActive(!request.prefix && hitsValue.length > 0 ? 0 : -1)
      },
      (error: unknown) => {
        if (mine !== sequence) return
        inFlight = undefined
        setLoadingMore(false)
        setFailure(searchFailureKind(error))
        setPhase('error')
      },
    )
  }

  const schedule = () => {
    if (timer !== undefined) clearTimeout(timer)
    timer = setTimeout(() => {
      timer = undefined
      execute(null)
    }, SEARCH_DEBOUNCE_MS)
  }

  return {
    query,
    scope: () => (scopeView() === 'thread' && !options.threadId() ? 'room' : scopeView()),
    personId: person,
    phase,
    hits,
    coverage,
    truncated,
    excluded,
    hasMore,
    loadingMore,
    failure,
    activeIndex,
    answeredQuery,
    setQuery(value) {
      const next = value.slice(0, SEARCH_MAX_CHARS)
      queryValue = next
      setQueryView(next)
      prefixMode = true
      if (next.trim().length < SEARCH_MIN_CHARS) {
        cancelPending()
        clearResults()
        setPhase('idle')
        return
      }
      setPhase('searching')
      schedule()
    },
    submit() {
      if (queryValue.trim().length < SEARCH_MIN_CHARS) return
      if (timer !== undefined) clearTimeout(timer)
      timer = undefined
      prefixMode = false
      execute(null)
    },
    setScope(scope) {
      scopeValue = scope
      if (scope !== 'person') personValue = null
      setScopeView(scope)
      setPersonView(personValue)
      if (queryValue.trim().length >= SEARCH_MIN_CHARS) {
        if (timer !== undefined) clearTimeout(timer)
        timer = undefined
        // A person scope waits for a person: `room` stands in until one is chosen.
        if (scope !== 'person') execute(null)
      }
    },
    setPerson(subjectId) {
      personValue = subjectId
      setPersonView(subjectId)
      if (queryValue.trim().length >= SEARCH_MIN_CHARS) execute(null)
    },
    setActiveIndex(index) {
      setActive(Math.max(-1, Math.min(index, hitsValue.length - 1)))
    },
    loadMore() {
      if (cursorValue && !inFlight) execute(cursorValue)
    },
    retry() {
      execute(null)
    },
    reset() {
      cancelPending()
      queryValue = ''
      scopeValue = 'room'
      personValue = null
      prefixMode = true
      setQueryView('')
      setScopeView('room')
      setPersonView(null)
      clearResults()
      setPhase('idle')
    },
    dispose() {
      cancelPending()
    },
  }
}
